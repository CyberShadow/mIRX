#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
states_dir="$root_dir/tests/ui-states"
baseline_dir="$root_dir/artifacts/ui-snapshots"

started_xvfb=0
xvfb_pid=
tmpdir=
app_pid=
target_dir=
ui_exe=
driver_exe=
generated_dir=
diff_dir=
keep_tmpdir=0
updated_cases=()
# The prefix of the 144-dpi cases, set up when one runs.
dpi144_prefix=

# Cases re-run at 144 dpi (150% display scaling) in a prefix of their own,
# as <case>-dpi144 with their own baselines: the base case's state and
# expectations, the default frame size (scaled, clamped to the screen) and
# --expect-dpi 144.
dpi144_cases=(multi-room image-message-loaded room-list-listing compose-color-index
  dialog-options-display dialog-options-irc dialog-colors dialog-sign-out-session dialog-log-out
  dialog-export-room-keys dialog-address-book-notify dialog-address-book-colors toolbar-connected
  line-marker dialog-channel-central dialog-sessions)

die() {
  echo "ui-snapshot: $*" >&2
  exit 1
}

# shellcheck source=wine-prefix.sh
source "$root_dir/scripts/wine-prefix.sh"

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

display_socket_path() {
  local display_value=${1#:}
  local display_number=${display_value%%.*}
  printf '/tmp/.X11-unix/X%s' "$display_number"
}

display_lock_path() {
  local display_value=${1#:}
  local display_number=${display_value%%.*}
  printf '/tmp/.X%s-lock' "$display_number"
}

display_is_healthy() {
  local display=$1
  DISPLAY=$display xdpyinfo >/dev/null 2>&1
}

wait_for_display() {
  local display=$1
  local socket_path
  socket_path=$(display_socket_path "$display")
  for _ in $(seq 1 50); do
    if [[ -S "$socket_path" ]] && display_is_healthy "$display"; then
      return 0
    fi
    sleep 0.1
  done
  return 1
}

cleanup_stale_display_files() {
  local display=$1
  rm -f "$(display_socket_path "$display")" "$(display_lock_path "$display")"
}

start_xvfb_auto() {
  local display_file
  display_file=$(mktemp "$tmpdir/xvfb-display.XXXXXX")
  Xvfb -displayfd 3 -screen 0 1280x1024x24 3>"$display_file" >"$tmpdir/xvfb.log" 2>&1 &
  xvfb_pid=$!
  started_xvfb=1

  local display_number=
  for _ in $(seq 1 50); do
    if [[ -s "$display_file" ]]; then
      display_number=$(tr -d '\n' <"$display_file")
      break
    fi
    if ! kill -0 "$xvfb_pid" >/dev/null 2>&1; then
      break
    fi
    sleep 0.1
  done

  rm -f "$display_file"
  [[ -n "$display_number" ]] || die "Xvfb did not publish a display number"

  export DISPLAY=":$display_number"
  wait_for_display "$DISPLAY" || die "Xvfb did not become healthy on $DISPLAY"
}

start_xvfb_explicit() {
  local display=$1

  if display_is_healthy "$display"; then
    die "Xvfb display already in use: $display"
  fi

  cleanup_stale_display_files "$display"
  export DISPLAY=$display
  Xvfb "$DISPLAY" -screen 0 1280x1024x24 >"$tmpdir/xvfb.log" 2>&1 &
  xvfb_pid=$!
  started_xvfb=1
  wait_for_display "$DISPLAY" || die "Xvfb did not become healthy on $DISPLAY"
}

ensure_xvfb() {
  if [[ -n "${MIRX_WINE_DISPLAY:-}" ]]; then
    start_xvfb_explicit "$MIRX_WINE_DISPLAY"
  else
    start_xvfb_auto
  fi
}

resolve_binaries() {
  target_dir="${CARGO_TARGET_DIR:-$root_dir/target}/x86_64-pc-windows-gnu/debug"
  ui_exe="$target_dir/mirx-ui.exe"
  driver_exe="$target_dir/mirx-snapshot-driver.exe"
}

unix_to_wine_path() {
  local unix_path=$1
  printf 'Z:%s' "${unix_path//\//\\}"
}

cleanup_app() {
  if [[ -n "${app_pid:-}" ]]; then
    kill "$app_pid" >/dev/null 2>&1 || true
    wait "$app_pid" 2>/dev/null || true
    app_pid=
  fi
}

cleanup() {
  cleanup_app
  if [[ -n "${dpi144_prefix:-}" ]]; then
    WINEPREFIX=$dpi144_prefix wineserver -k >/dev/null 2>&1 || true
  fi
  if [[ "${started_xvfb:-0}" -eq 1 ]] && [[ -n "${xvfb_pid:-}" ]]; then
    kill "$xvfb_pid" >/dev/null 2>&1 || true
    wait "$xvfb_pid" 2>/dev/null || true
  fi
  if [[ "${keep_tmpdir:-0}" -eq 1 ]]; then
    echo "preserved UI snapshot outputs: $tmpdir" >&2
  elif [[ -n "${tmpdir:-}" ]]; then
    rm -rf "$tmpdir"
  fi
}

# Snapshots show what a user sees, so this prefix (and only this one: the
# automation tests' geometry must not change) differs from the default:
# - Decorated=N: Xvfb runs no window manager, and Wine leaves the title bars
#   of managed windows to one; undecorated, Wine draws captions itself.
# - CursorBlinkRate=-1: Wine reads it with wcstoul, so the caret's timer is
#   ~25 days long and a focused edit's caret is drawn once and never blinks,
#   which would make dialog snapshots pixel-flaky.
configure_snapshot_prefix() {
  wine reg add 'HKCU\Software\Wine\X11 Driver' /v Decorated /d N /f >/dev/null 2>&1 \
    || die "failed to set Wine's Decorated=N"
  wine reg add 'HKCU\Control Panel\Desktop' /v CursorBlinkRate /d -1 /f >/dev/null 2>&1 \
    || die "failed to set CursorBlinkRate=-1"
}

# A snapshot prefix whose screen is 144 dpi. Wine reads LogPixels when its
# server starts, so the server set up with it is stopped.
configure_dpi144_prefix() {
  dpi144_prefix=$tmpdir/wineprefix-dpi144
  (
    export WINEPREFIX=$dpi144_prefix
    ensure_wineprefix "$WINEPREFIX"
    configure_snapshot_prefix
    wine reg add 'HKCU\Control Panel\Desktop' /v LogPixels /t REG_DWORD /d 144 /f >/dev/null 2>&1 \
      || die "failed to set LogPixels=144"
    wineserver -k
    wineserver -w
  )
}

is_dpi144_case() {
  local base=${1%-dpi144}
  [[ $1 == "$base-dpi144" ]] || return 1
  local listed=
  for listed in "${dpi144_cases[@]}"; do
    [[ $listed == "$base" ]] && return 0
  done
  return 1
}

# Cargo builds are incremental, so this rebuilds exactly the stale binaries.
ensure_binaries() {
  cargo build --target x86_64-pc-windows-gnu -p mirx-ui
  cargo build --target x86_64-pc-windows-gnu -p mirx-testkit --bin mirx-snapshot-driver
}

is_default_snapshot_case() {
  return 0
}

configure_case_expectations() {
  local case_name=$1
  room_activity_expect_args=()
  room_expect_args=()
  room_membership_expect_args=()
  room_forbid_args=()
  room_direct_expect_args=()
  room_tag_expect_args=()
  room_read_target_expect_args=()
  room_unread_expect_args=()
  room_view_expect_args=()
  view_expect_args=()
  view_forbid_args=()
  room_member_expect_args=()
  status_view_expect_args=()
  status_view_forbid_args=()
  status_bar_expect_args=()
  status_bar_forbid_args=()
  switchbar_item_expect_args=()
  switchbar_no_item_expect_args=()
  switchbar_activity_expect_args=()
  switchbar_no_activity_expect_args=()
  navigation_expect_args=()
  navigation_forbid_args=()
  navigation_activity_expect_args=()
  navigation_no_activity_expect_args=()
  navigation_node_expect_args=()
  expect_selected=
  expect_error=
  expect_verification=
  expect_verification_state=
  expect_active_view=
  select_views=()
  trigger_history_backfill=
  append_events_path=
  find_active_view_query=
  ordered_actions=()
  expect_active_find_history=
  expect_active_find_selected_text=
  case_frame_size=
  expect_switchbar_active=
  expect_switchbar_rows=
  frame_bar_hidden_expect_args=()
  toolbar_button_expect_args=()
  verification_text_expect_args=()
  mdi_placement_expect_args=()
  mdi_title_expect_args=()
  expect_dialog_title=

  case "$case_name" in
    mdi-tiled|mdi-cascaded|mdi-minimized)
      expect_selected='!lobby:example'
      expect_active_view='room:!lobby:example'
      expect_switchbar_active='room:!lobby:example'
      room_view_expect_args=(
        --expect-room-view '!lobby:example::<Bob> coffee is ready'
        --expect-room-view '!ops:example::<Bob> rollback plan is in the doc'
      )
      room_member_expect_args=(
        --expect-room-member '!lobby:example::Carol'
        --expect-room-member '!ops:example::Alice'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!ops:example::message'
      )
      mdi_title_expect_args=(
        --expect-mdi-title 'status::Status'
        --expect-mdi-title 'room:!lobby:example::Lobby: welcome to the lobby'
        --expect-mdi-title 'room:!ops:example::Ops: incident review'
      )
      case "$case_name" in
        mdi-tiled)
          ordered_actions=(arrange:tile_vertical)
          mdi_placement_expect_args=(
            --expect-mdi-placement 'status::normal'
            --expect-mdi-placement 'room:!lobby:example::normal'
            --expect-mdi-placement 'room:!ops:example::normal'
          )
          ;;
        mdi-cascaded)
          ordered_actions=(arrange:cascade)
          mdi_placement_expect_args=(
            --expect-mdi-placement 'status::normal'
            --expect-mdi-placement 'room:!lobby:example::normal'
            --expect-mdi-placement 'room:!ops:example::normal'
          )
          ;;
        mdi-minimized)
          # Tiling again leaves the bottom row of the MDI client to icons.
          ordered_actions=(
            arrange:tile_vertical
            'window-state:room:!ops:example=minimized'
            arrange:tile_vertical
          )
          mdi_placement_expect_args=(
            --expect-mdi-placement 'status::normal'
            --expect-mdi-placement 'room:!lobby:example::normal'
            --expect-mdi-placement 'room:!ops:example::minimized'
          )
          ;;
      esac
      ;;

    # Each dialog opens over the same small room window.
    dialog-*)
      expect_selected='!topic:example'
      expect_active_view='room:!topic:example'
      case "$case_name" in
        dialog-options-connect)
          ordered_actions=(dialog:options:Connect/Options)
          expect_dialog_title='mirx Options'
          ;;
        dialog-options-irc)
          ordered_actions=(dialog:options:IRC)
          expect_dialog_title='mirx Options'
          ;;
        dialog-options-messages)
          ordered_actions=(dialog:options:IRC/Messages)
          expect_dialog_title='mirx Options'
          ;;
        dialog-options-highlight)
          ordered_actions=(dialog:options:IRC/Highlight)
          expect_dialog_title='mirx Options'
          ;;
        dialog-options-logging)
          ordered_actions=(dialog:options:IRC/Logging)
          expect_dialog_title='mirx Options'
          ;;
        dialog-options-display)
          ordered_actions=(dialog:options:Display)
          expect_dialog_title='mirx Options'
          ;;
        dialog-options-tray)
          ordered_actions=(dialog:options:Display/Tray)
          expect_dialog_title='mirx Options'
          ;;
        dialog-options-tips)
          ordered_actions=(dialog:options:Display/Tips)
          expect_dialog_title='mirx Options'
          ;;
        dialog-options-other)
          ordered_actions=(dialog:options:Other)
          expect_dialog_title='mirx Options'
          ;;
        dialog-colors)
          ordered_actions=(dialog:colors)
          expect_dialog_title='mirx Colors'
          ;;
        dialog-aliases)
          ordered_actions=(dialog:aliases)
          expect_dialog_title='Aliases - aliases.ini'
          ;;
        dialog-address-book-notify)
          # Tools > Address Book at its Notify tab: one entry performs
          # /whois (+) and has a note, one names a stranger by ID.
          ordered_actions=(
            'input:/notify +bob lunch buddy'
            'input:/notify @dave:example'
            dialog:address_book
          )
          expect_dialog_title='Address Book'
          ;;
        dialog-address-book-control)
          # /abook -c: the Control tab, with the server's ignore list.
          ordered_actions=(
            'compose-text:/abook -c'
            dialog:compose_return
          )
          expect_dialog_title='Address Book'
          ;;
        dialog-address-book-colors)
          # /abook -l: the Colors tab, with Bob's entry in red for his
          # modes, and anyone on the notify list in an auto colour.
          ordered_actions=(
            'input:/cnick bob 4 @&~'
            'input:/cnick -y *:example *'
            'compose-text:/abook -l'
            dialog:compose_return
          )
          expect_dialog_title='Address Book'
          ;;
        dialog-channel-central)
          # /channel, and the core's answer: two bans, the topic set by ops
          # only, invite only and in the Rooms List; everything the
          # user's (@me, an administrator) to change. Topic History has the
          # current topic and the earlier one the window saw set.
          ordered_actions=(
            'compose-text:/channel'
            dialog:compose_return
            "append:$root_dir/tests/ui-state-updates/dialog-channel-central.json"
          )
          expect_dialog_title='Channel Central - Topic Room'
          ;;
        dialog-channel-central-not-op)
          # A member who may set only the topic (members may: Only ops set
          # topic unchecked): the rest greyed, and the line over OK says
          # why. Knock is neither invite only nor not: Invite only is
          # indeterminate.
          ordered_actions=(
            'compose-text:/channel'
            dialog:compose_return
            "append:$root_dir/tests/ui-state-updates/dialog-channel-central-not-op.json"
          )
          expect_dialog_title='Channel Central - Topic Room'
          ;;
        dialog-select-server)
          ordered_actions=(dialog:select_server)
          expect_dialog_title='Select Server'
          ;;
        dialog-about)
          ordered_actions=(dialog:about)
          expect_dialog_title='About mirx'
          ;;
        dialog-commands-join)
          ordered_actions=(dialog:commands_join)
          expect_dialog_title='Join room'
          ;;
        dialog-recover-e2ee)
          ordered_actions=(dialog:recover_e2ee)
          expect_dialog_title='Recover E2EE'
          ;;
        dialog-setup-e2ee)
          ordered_actions=(dialog:setup_e2ee_recovery)
          expect_dialog_title='Set Up E2EE Recovery'
          ;;
        dialog-reset-e2ee)
          # A password session: the dialog asks for the account password.
          ordered_actions=(dialog:reset_e2ee)
          expect_dialog_title='Reset E2EE Identity'
          ;;
        dialog-sessions)
          # Tools > Sessions..., and the core's answer: this session, an
          # unverified phone, and a session the server has no time for.
          ordered_actions=(
            dialog:sessions
            "append:$root_dir/tests/ui-state-updates/dialog-sessions.json"
          )
          expect_dialog_title='Sessions - @me:example'
          ;;
        dialog-sessions-not-verified)
          # This session is not verified: the line under the list says so.
          ordered_actions=(
            dialog:sessions
            "append:$root_dir/tests/ui-state-updates/dialog-sessions-not-verified.json"
          )
          expect_dialog_title='Sessions - @me:example'
          ;;
        dialog-sign-out-session)
          # /devices delete of a password session asks for the password.
          ordered_actions=(
            'compose-text:/devices delete PH0NE'
            dialog:compose_return
          )
          expect_dialog_title='Sign Out Session'
          ;;
        dialog-log-out)
          # The core's answer to File > Log Out without recovery: the
          # warning opens by itself.
          append_events_path="$root_dir/tests/ui-state-updates/dialog-log-out.json"
          ordered_actions=(dialog:opened)
          expect_dialog_title='Log Out'
          ;;
        dialog-export-room-keys)
          # A file given: the passphrase dialog, asked twice.
          ordered_actions=(
            'compose-text:/e2ee export C:\Users\me\Documents\mirx-keys.txt'
            dialog:compose_return
          )
          expect_dialog_title='Export Room Keys'
          ;;
        dialog-import-room-keys)
          ordered_actions=(
            'compose-text:/e2ee import C:\Users\me\Documents\mirx-keys.txt'
            dialog:compose_return
          )
          expect_dialog_title='Import Room Keys'
          ;;
        dialog-close-confirm)
          ordered_actions=(dialog:close_active_view)
          expect_dialog_title='mirx'
          ;;
        *)
          die "no expectations configured for snapshot case: $case_name"
          ;;
      esac
      ;;

    room-topic)
      expect_selected='!topic:example'
      expect_active_view='room:!topic:example'
      room_expect_args=(
        --expect-room '!topic:example'
      )
      room_view_expect_args=(
        --expect-room-view '!topic:example::* Now talking in Topic Room (!topic:example)'
        --expect-room-view "!topic:example::* Topic is 'incident review'"
      )
      mdi_title_expect_args=(
        --expect-mdi-title 'room:!topic:example::Topic Room: incident review'
      )
    ;;

    # The lines a room's name, join rule and encryption changes print, in
    # the Topic and Settings colours.
    room-settings-lines)
      expect_selected='!topic:example'
      expect_active_view='room:!topic:example'
      room_view_expect_args=(
        --expect-room-view '!topic:example::* Alice turns on end-to-end encryption'
        --expect-room-view "!topic:example::* Alice changes room name to 'Topic Room'"
        --expect-room-view '!topic:example::* Alice removes room name'
        --expect-room-view '!topic:example::* Alice makes the room public'
        --expect-room-view '!topic:example::* Alice makes the room invite only'
        --expect-room-view '!topic:example::* Alice makes the room ask to join'
        --expect-room-view '!topic:example::* Alice makes the room open to members of its spaces'
        --expect-room-view '!topic:example::* Alice makes the room ask to join, or open to members of its spaces'
        --expect-room-view '!topic:example::* Alice makes the room private'
        --expect-room-view '!topic:example::* Alice sets the join rule to xyz.custom'
      )
      ;;

    # Ctrl+K over the editbox of a small room window: the colour index.
    compose-color-index)
      expect_selected='!topic:example'
      expect_active_view='room:!topic:example'
      ordered_actions=('compose-text:the fix is ' compose-key:color)
      ;;

    multi-room)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      room_expect_args=(
        --expect-room '!lobby:example'
        --expect-room '!ops:example'
        --expect-room '!random:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!ops:example::incident review'
        --expect-room-activity '!ops:example::carol'
        --expect-room-activity '!ops:example::join'
        --expect-room-activity '!ops:example::@alice:example'
        --expect-room-activity '!ops:example::deploy window opens'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::* Now talking in Ops (!ops:example)'
        --expect-room-view "!ops:example::* Topic is 'incident review'"
        --expect-room-view '!ops:example::* Messages in this room are end-to-end encrypted'
        --expect-room-view "!ops:example::[22:13] * Alice changes topic to 'incident review'"
        --expect-room-view '!ops:example::[22:14] * Joins: Carol (@carol:example)'
        --expect-room-view '!ops:example::[22:15] <Alice> ops topic: incident review'
      )
      mdi_title_expect_args=(
        --expect-mdi-title 'room:!ops:example::Ops: incident review'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 3 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ops:example::Ops'
        --expect-switchbar-item 'room:!lobby:example::Lobby'
        --expect-switchbar-item 'room:!random:example::Random'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!lobby:example::message'
        --expect-switchbar-activity 'room:!random:example::message'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!ops:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!ops:example'
      navigation_forbid_args=(
        --expect-no-navigation-text 'incident review'
        --expect-no-navigation-text 'encryption enabled'
        --expect-no-navigation-text 'deploy window opens'
      )
      # Never signed in: Connect starts the sign-in. Disconnect, shown
      # only when it applies, keeps its button enabled.
      toolbar_button_expect_args=(
        --expect-toolbar-button 'Connect::shown::enabled'
        --expect-toolbar-button 'Disconnect::hidden::enabled'
      )
      ;;
    invited-room-list)
      expect_selected='!invite:example'
      expect_active_view='room:!invite:example'
      select_views=('room:!invite:example')
      room_expect_args=(
        --expect-room '!beta:example'
        --expect-room '!invite:example'
      )
      room_membership_expect_args=(
        --expect-room-membership '!beta:example::join'
        --expect-room-membership '!invite:example::invite'
      )
      room_view_expect_args=(
        --expect-room-view '!invite:example::* Bob (@bob:example) invites you to join Lobby'
        --expect-room-view '!invite:example::Use /accept or /reject to respond.'
      )
      status_bar_expect_args=(
        --expect-status-bar 'invited: Lobby (!invite:example)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!beta:example::Beta'
        --expect-switchbar-item 'room:!invite:example::Lobby [invite]'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!beta:example'
        --expect-no-switchbar-activity 'room:!invite:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!invite:example'
      navigation_expect_args=(
        --expect-navigation-text 'Beta'
        --expect-navigation-text 'Lobby [invite]'
      )
      ;;
    invited-direct-room)
      expect_selected='!invite-direct:example'
      expect_active_view='room:!invite-direct:example'
      select_views=('room:!invite-direct:example')
      room_expect_args=(
        --expect-room '!beta:example'
        --expect-room '!invite-direct:example'
      )
      room_membership_expect_args=(
        --expect-room-membership '!beta:example::join'
        --expect-room-membership '!invite-direct:example::invite'
      )
      room_direct_expect_args=(
        --expect-direct-room '!invite-direct:example'
      )
      room_view_expect_args=(
        --expect-room-view '!invite-direct:example::* Daria (@daria:example) invites you to a query'
        --expect-room-view '!invite-direct:example::Use /accept or /reject to respond.'
      )
      status_bar_expect_args=(
        --expect-status-bar 'invited: Daria (!invite-direct:example)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!beta:example::Beta'
        --expect-switchbar-item 'room:!invite-direct:example::Daria [invite]'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!beta:example'
        --expect-no-switchbar-activity 'room:!invite-direct:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!invite-direct:example'
      navigation_expect_args=(
        --expect-navigation-text 'Beta'
        --expect-navigation-text 'Daria [invite]'
      )
      ;;
    invited-room-accept)
      expect_selected='!invite:example'
      expect_active_view='room:!invite:example'
      select_views=('room:!invite:example')
      append_events_path="$root_dir/tests/ui-state-updates/invited-room-accept.json"
      room_expect_args=(
        --expect-room '!beta:example'
        --expect-room '!invite:example'
      )
      room_membership_expect_args=(
        --expect-room-membership '!beta:example::join'
        --expect-room-membership '!invite:example::join'
      )
      room_view_expect_args=(
        --expect-room-view '!invite:example::* Now talking in Lobby (!invite:example)'
      )
      view_forbid_args=(
        --expect-no-view 'room:!invite:example::Use /accept or /reject to respond.'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!beta:example::Beta'
        --expect-switchbar-item 'room:!invite:example::Lobby'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!beta:example'
        --expect-no-switchbar-activity 'room:!invite:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!invite:example'
      navigation_expect_args=(
        --expect-navigation-text 'Beta'
        --expect-navigation-text 'Lobby'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text 'Lobby [invite]'
      )
      ;;
    invited-room-reject)
      expect_selected='!beta:example'
      expect_active_view='room:!beta:example'
      select_views=('room:!invite:example')
      append_events_path="$root_dir/tests/ui-state-updates/invited-room-reject.json"
      room_expect_args=(
        --expect-room '!beta:example'
      )
      room_membership_expect_args=(
        --expect-room-membership '!beta:example::join'
      )
      room_forbid_args=(
        --expect-no-room '!invite:example'
      )
      room_view_expect_args=(
        --expect-room-view '!beta:example::* Now talking in Beta (!beta:example)'
        --expect-room-view '!beta:example::beta latest'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!beta:example::Beta'
      )
      switchbar_no_item_expect_args=(
        --expect-no-switchbar-item 'room:!invite:example'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!beta:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!beta:example'
      navigation_expect_args=(
        --expect-navigation-text 'Beta'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text 'Lobby [invite]'
      )
      ;;
    treebar-status-root)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      room_view_expect_args=(
        --expect-room-view '!ops:example::the treebar roots at the Status window'
      )
      mdi_title_expect_args=(
        --expect-mdi-title 'status::Status: @me:example'
        --expect-mdi-title 'room:!ops:example::Ops'
        --expect-mdi-title 'room:!alice:example::Alice (@alice:example)'
        --expect-mdi-title 'room:!invite:example::Invited [invite]'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::@me:example'
        --expect-switchbar-item 'room:!ops:example::Ops'
        --expect-switchbar-item 'room:!shared:example::Shared'
        --expect-switchbar-item 'room:!lobby:example::Lobby'
        --expect-switchbar-item 'room:!alice:example::Alice'
        --expect-switchbar-item 'room:!invite:example::Invited [invite]'
      )
      switchbar_no_item_expect_args=(
        --expect-no-switchbar-item 'room:!work:example'
      )
      expect_switchbar_active='room:!ops:example'
      navigation_expect_args=(
        --expect-navigation-text '@me:example'
        --expect-navigation-text 'Work'
        --expect-navigation-text 'Alice'
        --expect-navigation-text 'Invited [invite]'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text '[e2ee]'
        --expect-no-navigation-text '[query'
        --expect-no-navigation-text 'rooms:'
        --expect-no-navigation-text 'invites:'
        --expect-no-navigation-text 'mirx'
      )
      ;;
    treebar-collapsed-space)
      # Work holds Ops (a message) and the nested Team space, whose Deploy
      # has a highlight; collapsed, Work is drawn in the highest of them.
      expect_selected='!lobby:example'
      expect_active_view='room:!lobby:example'
      expect_switchbar_active='room:!lobby:example'
      ordered_actions=('collapse:space:!work:example')
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!ops:example::message'
        --expect-switchbar-activity 'room:!deploy:example::highlight'
        --expect-switchbar-activity 'room:!random:example::message'
      )
      navigation_node_expect_args=(
        --expect-navigation-node 'space:!work:example::collapsed::highlight'
        --expect-navigation-node 'space:!team:example::expanded::none'
        --expect-navigation-node 'space:!social:example::expanded::none'
        --expect-navigation-node 'status::expanded::none'
      )
      navigation_activity_expect_args=(
        --expect-navigation-activity 'room:!random:example::message'
      )
      ;;
    space-hierarchy)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      room_expect_args=(
        --expect-room '!work:example'
        --expect-room '!social:example'
        --expect-room '!ops:example'
        --expect-room '!shared:example'
        --expect-room '!random:example'
        --expect-room '!direct:example'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::* Now talking in Ops (!ops:example)'
        --expect-room-view '!direct:example::* Now talking in Direct (!direct:example)'
      )
      room_direct_expect_args=(
        --expect-direct-room '!direct:example'
      )
      room_tag_expect_args=(
        --expect-room-tag '!direct:example::m.favourite::0.25'
        --expect-room-tag '!direct:example::u.work::null'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ops:example::Ops'
        --expect-switchbar-item 'room:!shared:example::Shared'
        --expect-switchbar-item 'room:!random:example::Random'
        --expect-switchbar-item 'room:!direct:example::Direct'
      )
      mdi_title_expect_args=(
        --expect-mdi-title 'room:!direct:example::Direct (@alice:example)'
      )
      switchbar_no_item_expect_args=(
        --expect-no-switchbar-item 'room:!work:example'
        --expect-no-switchbar-item 'room:!social:example'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!shared:example::message'
        --expect-switchbar-activity 'room:!random:example::message'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!ops:example'
        --expect-no-switchbar-activity 'room:!direct:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!ops:example'
      navigation_expect_args=(
        --expect-navigation-text 'Work'
        --expect-navigation-text 'Social'
        --expect-navigation-text 'Ops'
        --expect-navigation-text 'Shared'
        --expect-navigation-text 'Random'
        --expect-navigation-text 'Direct'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text '[e2ee]'
        --expect-no-navigation-text '[query'
        --expect-no-navigation-text 'rooms:'
        --expect-no-navigation-text 'invites:'
        --expect-no-navigation-text 'ops workspace is ready'
        --expect-no-navigation-text 'shared room has updates'
        --expect-no-navigation-text 'random room has updates'
      )
      ;;
    thread-hierarchy)
      expect_selected='!ops:example'
      expect_active_view='thread:!ops:example|$thread-root'
      select_views=('thread:!ops:example|$thread-root')
      room_expect_args=(
        --expect-room '!work:example'
        --expect-room '!ops:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!ops:example::"type":"thread"'
        --expect-room-activity '!ops:example::$thread-root'
        --expect-room-activity '!ops:example::pending threaded draft'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::lobby'
        --expect-room-view '!ops:example::root msg'
        --expect-room-view '!ops:example::thread rep'
        --expect-room-view '!ops:example::pending threaded draft'
      )
      view_expect_args=(
        --expect-view 'thread:!ops:example|$thread-root::root msg'
        --expect-view 'thread:!ops:example|$thread-root::thread rep'
        --expect-view 'thread:!ops:example|$thread-root::pending threaded draft'
      )
      view_forbid_args=(
        --expect-no-view 'thread:!ops:example|$thread-root::lobby'
        --expect-no-view 'thread:!ops:example|$thread-root::plain'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ops:example::Ops'
        --expect-switchbar-item 'thread:!ops:example|$thread-root::root msg'
      )
      switchbar_no_item_expect_args=(
        --expect-no-switchbar-item 'room:!work:example'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!ops:example'
        --expect-no-switchbar-activity 'thread:!ops:example|$thread-root'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='thread:!ops:example|$thread-root'
      navigation_expect_args=(
        --expect-navigation-text 'Work'
        --expect-navigation-text 'Ops'
        --expect-navigation-text 'root msg'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text 'plain'
      )
      ;;
    find-room)
      expect_selected='!find-room:example'
      expect_active_view='room:!find-room:example'
      find_active_view_query='needle'
      expect_active_find_history='local_only'
      room_expect_args=(
        --expect-room '!find-room:example'
      )
      room_view_expect_args=(
        --expect-room-view '!find-room:example::needle in the first matching row'
        --expect-room-view '!find-room:example::needle appears again in the second matching row'
      )
      status_bar_expect_args=(
        --expect-status-bar 'find: 1/2 "needle"'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!find-room:example::Find Room'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
        --expect-no-switchbar-activity 'room:!find-room:example'
      )
      expect_switchbar_active='room:!find-room:example'
      navigation_forbid_args=(
        --expect-no-navigation-text 'needle in the first matching row'
      )
      ;;
    find-room-history-loading)
      expect_selected='!find-history:example'
      expect_active_view='room:!find-history:example'
      find_active_view_query='history needle'
      expect_active_find_history='loading'
      room_view_expect_args=(
        --expect-room-view '!find-history:example::* Loading older history...'
      )
      status_bar_expect_args=(
        --expect-status-bar 'find: loading older history for "history needle"'
      )
      expect_switchbar_active='room:!find-history:example'
      ;;
    find-room-history-match)
      expect_selected='!find-history:example'
      expect_active_view='room:!find-history:example'
      ordered_actions=(
        'find:history needle'
        "append:$root_dir/tests/ui-state-updates/find-room-history-match.json"
      )
      expect_active_find_history='local_only'
      expect_active_find_selected_text='older history needle match'
      room_view_expect_args=(
        --expect-room-view '!find-history:example::older history needle match'
      )
      status_bar_expect_args=(
        --expect-status-bar 'find: 1/1 "history needle"'
      )
      expect_switchbar_active='room:!find-history:example'
      ;;
    find-room-history-exhausted)
      expect_selected='!find-history:example'
      expect_active_view='room:!find-history:example'
      ordered_actions=(
        'find:history needle'
        "append:$root_dir/tests/ui-state-updates/find-room-history-exhausted.json"
      )
      expect_active_find_history='exhausted'
      status_bar_expect_args=(
        --expect-status-bar 'find: no matches for "history needle" in loaded or older history'
      )
      expect_switchbar_active='room:!find-history:example'
      ;;
    find-room-history-failed)
      expect_selected='!find-history:example'
      expect_active_view='room:!find-history:example'
      ordered_actions=(
        'find:history needle'
        "append:$root_dir/tests/ui-state-updates/find-room-history-failed.json"
      )
      expect_active_find_history='failed'
      status_bar_expect_args=(
        --expect-status-bar 'find: unable to load older history for "history needle": homeserver unavailable'
      )
      expect_switchbar_active='room:!find-history:example'
      ;;
    find-thread)
      expect_selected='!find-thread:example'
      expect_active_view='thread:!find-thread:example|$find-thread-root'
      select_views=('thread:!find-thread:example|$find-thread-root')
      find_active_view_query='thread needle'
      room_expect_args=(
        --expect-room '!find-thread:example'
      )
      room_view_expect_args=(
        --expect-room-view '!find-thread:example::root msg'
        --expect-room-view '!find-thread:example::thread needle alpha'
        --expect-room-view '!find-thread:example::thread needle beta'
      )
      view_expect_args=(
        --expect-view 'thread:!find-thread:example|$find-thread-root::thread needle alpha'
        --expect-view 'thread:!find-thread:example|$find-thread-root::thread needle beta'
      )
      view_forbid_args=(
        --expect-no-view 'thread:!find-thread:example|$find-thread-root::room-only lobby note'
      )
      status_bar_expect_args=(
        # The second reply quotes the first: the quote is window text too.
        --expect-status-bar 'find: 1/3 "thread needle"'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!find-thread:example::Find Thread'
        --expect-switchbar-item 'thread:!find-thread:example|$find-thread-root::root msg'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
        --expect-no-switchbar-activity 'room:!find-thread:example'
        --expect-no-switchbar-activity 'thread:!find-thread:example|$find-thread-root'
      )
      expect_switchbar_active='thread:!find-thread:example|$find-thread-root'
      navigation_expect_args=(
        --expect-navigation-text 'Find Thread'
        --expect-navigation-text 'root msg'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text 'room-only lobby note'
      )
      ;;
    find-thread-history-match)
      expect_selected='!find-thread:example'
      expect_active_view='thread:!find-thread:example|$find-thread-root'
      select_views=('thread:!find-thread:example|$find-thread-root')
      ordered_actions=(
        'find:history needle'
        "append:$root_dir/tests/ui-state-updates/find-thread-history-match.json"
      )
      expect_active_find_history='local_only'
      expect_active_find_selected_text='older thread history needle'
      view_expect_args=(
        --expect-view 'thread:!find-thread:example|$find-thread-root::older thread history needle'
      )
      view_forbid_args=(
        --expect-no-view 'thread:!find-thread:example|$find-thread-root::older room-only history needle'
      )
      status_bar_expect_args=(
        --expect-status-bar 'find: 1/1 "history needle"'
      )
      expect_switchbar_active='thread:!find-thread:example|$find-thread-root'
      ;;
    find-status)
      expect_active_view='status'
      find_active_view_query='flow-verify-request'
      expect_verification='flow-verify-request'
      expect_verification_state='requested'
      verification_text_expect_args=(
        --expect-verification-text 'flow_id=flow-verify-request'
        --expect-verification-text 'user_id=@alice:example'
        --expect-verification-text 'state=requested'
      )
      status_view_expect_args=(
        --expect-status-view '[22:12] * Verification requested by @alice:example (flow flow-verify-request)'
      )
      status_bar_expect_args=(
        --expect-status-bar 'find: 1/1 "flow-verify-request"'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='status'
      navigation_forbid_args=(
        --expect-no-navigation-text 'flow-verify-request'
      )
      ;;
    toolbar-connected)
      # The multi-room fixture signed in and syncing: the toolbar swaps
      # Connect for Disconnect.
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      room_expect_args=(
        --expect-room '!lobby:example'
        --expect-room '!ops:example'
        --expect-room '!random:example'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::[22:15] <Alice> ops topic: incident review'
      )
      toolbar_button_expect_args=(
        --expect-toolbar-button 'Connect::hidden::disabled'
        --expect-toolbar-button 'Disconnect::shown::enabled'
      )
      ;;
    sign-in-browser)
      # A replayed load: the page is printed, not opened.
      expect_active_view='status'
      select_views=('status')
      status_view_expect_args=(
        --expect-status-view '[22:12] * Sign in on the page your browser opened; File > Disconnect cancels'
        --expect-status-view '[22:12] * If no browser opened, go to https://account.example.org/oauth2/authorize?response_type=code&client_id=01JMIRXCLIENT&state=bXlzdGF0ZQ&redirect_uri=http%3A%2F%2F127.0.0.1%3A49152%2FkOGorr0udgDnscqBELcVTQ&scope=urn%3Amatrix%3Aorg.matrix.msc2967.client%3Aapi%3A%2A'
      )
      status_view_forbid_args=(
        --expect-no-status-view 'Unable to open'
      )
      status_bar_expect_args=(
        --expect-status-bar 'waiting for browser sign-in'
      )
      # Disconnect cancels the sign-in.
      toolbar_button_expect_args=(
        --expect-toolbar-button 'Connect::hidden::disabled'
        --expect-toolbar-button 'Disconnect::shown::enabled'
      )
      mdi_title_expect_args=(
        --expect-mdi-title 'status::Status'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
      )
      expect_switchbar_active='status'
      ;;
    logging-out)
      # A Log Out under way: Disconnect stops it.
      expect_active_view='status'
      select_views=('status')
      status_view_expect_args=(
        --expect-status-view '[22:12] * Logged in as @me:example (device MIRXDEVICE)'
        --expect-status-view '[22:12] * Syncing with the homeserver'
        --expect-status-view '[22:12] * Synced rooms: 1 joined, 0 invited'
        --expect-status-view '[22:12] * Logging out; File > Disconnect stops it'
      )
      status_bar_expect_args=(
        --expect-status-bar 'logging out'
      )
      toolbar_button_expect_args=(
        --expect-toolbar-button 'Connect::hidden::disabled'
        --expect-toolbar-button 'Disconnect::shown::enabled'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::@me:example'
        --expect-switchbar-item 'room:!ops:example::Ops'
      )
      expect_switchbar_active='status'
      ;;
    notify-lines)
      # The notify list: users added while on are announced at once, then
      # who comes and goes, in the Notify colour; the listing after.
      expect_active_view='status'
      select_views=('status')
      ordered_actions=(
        'input:/notify bob lunch buddy'
        'input:/notify carol'
        'input:/notify @dave:example'
        "append:$root_dir/tests/ui-state-updates/notify-lines.json"
        'input:/notify'
      )
      status_view_expect_args=(
        --expect-status-view '[22:12] * Added Bob (@bob:example) to notify list'
        --expect-status-view '[22:12] * Bob is on Matrix'
        --expect-status-view '[22:12] * Carol is on Matrix'
        --expect-status-view '[22:12] * Added @dave:example to notify list'
        --expect-status-view '[22:12] * Bob has left Matrix'
        --expect-status-view '[22:12] * @dave:example is on Matrix'
        --expect-status-view '[22:12] *   Bob (@bob:example) is not on Matrix - lunch buddy'
        --expect-status-view '[22:12] *   Carol (@carol:example) is on Matrix'
        --expect-status-view '[22:12] *   @dave:example is on Matrix'
      )
      status_bar_expect_args=(
        --expect-status-bar 'notify list saved'
      )
      expect_switchbar_active='status'
      ;;
    status-log)
      expect_active_view='status'
      select_views=('status')
      # Tiled, so the Status caption shows.
      ordered_actions=(arrange:tile_vertical)
      expect_error='message send failed'
      status_view_expect_args=(
        --expect-status-view '[22:12] * Logged in as @me:example (device MIRXDEVICE)'
        --expect-status-view '[22:12] * Syncing with the homeserver'
        --expect-status-view '[22:12] * Synced rooms: 1 joined, 0 invited'
        --expect-status-view '[22:12] * E2EE: recovery enabled, local backup enabled, server backup not checked, cross-signing complete, this session verified'
        --expect-status-view '[22:12] * Error during send text: message send failed'
      )
      status_view_forbid_args=(
        --expect-no-status-view 'fixture-access-token'
        --expect-no-status-view 'status: '
      )
      mdi_title_expect_args=(
        --expect-mdi-title 'status::Status: @me:example'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::@me:example'
        --expect-switchbar-item 'room:!ops:example::Ops'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='status'
      ;;
    find-no-match)
      expect_selected='!find-miss:example'
      expect_active_view='room:!find-miss:example'
      find_active_view_query='missing needle'
      expect_active_find_history='loading'
      room_expect_args=(
        --expect-room '!find-miss:example'
      )
      room_view_expect_args=(
        --expect-room-view '!find-miss:example::alpha status update'
        --expect-room-view '!find-miss:example::beta follow-up'
      )
      status_bar_expect_args=(
        --expect-status-bar 'find: loading older history for "missing needle"'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!find-miss:example::Find Miss'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
        --expect-no-switchbar-activity 'room:!find-miss:example'
      )
      expect_switchbar_active='room:!find-miss:example'
      navigation_forbid_args=(
        --expect-no-navigation-text 'missing needle'
      )
      ;;
    thread-activity-indicators)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      select_views=('thread:!ops:example|$thread-root' 'room:!ops:example')
      append_events_path="$root_dir/tests/ui-state-updates/thread-activity-indicators.json"
      room_expect_args=(
        --expect-room '!work:example'
        --expect-room '!ops:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!ops:example::"type":"thread"'
        --expect-room-activity '!ops:example::$thread-root'
        --expect-room-activity '!ops:example::fresh thread activity'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::lobby'
        --expect-room-view '!ops:example::root msg'
        --expect-room-view '!ops:example::thread rep'
        --expect-room-view '!ops:example::fresh thread activity'
      )
      view_expect_args=(
        --expect-view 'thread:!ops:example|$thread-root::root msg'
        --expect-view 'thread:!ops:example|$thread-root::thread rep'
        --expect-view 'thread:!ops:example|$thread-root::fresh thread activity'
      )
      view_forbid_args=(
        --expect-no-view 'thread:!ops:example|$thread-root::lobby'
        --expect-no-view 'thread:!ops:example|$thread-root::plain'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ops:example::Ops'
        --expect-switchbar-item 'thread:!ops:example|$thread-root::root msg'
      )
      switchbar_no_item_expect_args=(
        --expect-no-switchbar-item 'room:!work:example'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'thread:!ops:example|$thread-root::message'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!ops:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!ops:example'
      navigation_expect_args=(
        --expect-navigation-text 'Ops'
        --expect-navigation-text 'root msg'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text 'plain'
      )
      ;;
    message-edits)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      select_views=('thread:!ops:example|$thread-root' 'room:!ops:example')
      append_events_path="$root_dir/tests/ui-state-updates/message-edits.json"
      room_expect_args=(
        --expect-room '!work:example'
        --expect-room '!ops:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!ops:example::lobby edited'
        --expect-room-activity '!ops:example::root msg edited'
        --expect-room-activity '!ops:example::thread reply'
        --expect-room-activity '!ops:example::"type":"thread"'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::* Now talking in Ops (!ops:example)'
        --expect-room-view '!ops:example::lobby edited'
        --expect-room-view '!ops:example::root msg edited'
        --expect-room-view '!ops:example::thread reply'
      )
      view_expect_args=(
        --expect-view 'thread:!ops:example|$thread-root::root msg edited'
        --expect-view 'thread:!ops:example|$thread-root::thread reply'
      )
      view_forbid_args=(
        --expect-no-view 'thread:!ops:example|$thread-root::lobby draft'
        --expect-no-view 'thread:!ops:example|$thread-root::root msg draft'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ops:example::Ops'
        --expect-switchbar-item 'thread:!ops:example|$thread-root::root msg edited'
      )
      switchbar_no_item_expect_args=(
        --expect-no-switchbar-item 'room:!work:example'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!ops:example'
        --expect-no-switchbar-activity 'thread:!ops:example|$thread-root'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!ops:example'
      navigation_expect_args=(
        --expect-navigation-text 'Work'
        --expect-navigation-text 'Ops'
        --expect-navigation-text 'root msg edited'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text 'root msg draft'
      )
      ;;
    reactions)
      expect_selected='!reactions:example'
      expect_active_view='room:!reactions:example'
      append_events_path="$root_dir/tests/ui-state-updates/reactions.json"
      select_views=('thread:!reactions:example|$react-thread-root' 'room:!reactions:example')
      room_expect_args=(
        --expect-room '!reactions:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!reactions:example::"reactions":['
        --expect-room-activity '!reactions:example::"key":"+1"'
        --expect-room-activity '!reactions:example::"count":4'
        --expect-room-activity '!reactions:example::$reaction-plus-one-b'
        --expect-room-activity '!reactions:example::"key":"eyes"'
        --expect-room-activity '!reactions:example::"type":"thread"'
      )
      room_view_expect_args=(
        --expect-room-view '!reactions:example::* Now talking in Reactions (!reactions:example)'
        --expect-room-view '!reactions:example::normal reaction target'
        --expect-room-view '!reactions:example::reactions: +1 Bob, Carol, @dave:example and 1 more'
      )
      view_expect_args=(
        --expect-view 'thread:!reactions:example|$react-thread-root::thread reaction root'
        --expect-view 'thread:!reactions:example|$react-thread-root::thread reaction reply'
        --expect-view 'thread:!reactions:example|$react-thread-root::reactions: eyes @dave:example'
      )
      view_forbid_args=(
        --expect-no-view 'room:!reactions:example::heart'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!reactions:example::Reactions'
        --expect-switchbar-item 'thread:!reactions:example|$react-thread-root::thread reaction root'
      )
      expect_switchbar_active='room:!reactions:example'
      navigation_expect_args=(
        --expect-navigation-text 'Reactions'
        --expect-navigation-text 'thread reaction root'
      )
      ;;
    replies)
      expect_selected='!replies:example'
      expect_active_view='room:!replies:example'
      ordered_actions=('input:/reply $replies-question')
      room_expect_args=(
        --expect-room '!replies:example'
      )
      room_view_expect_args=(
        --expect-room-view '!replies:example::    > <Alice> are we deploying today? the build looks green on eve...'
        --expect-room-view '!replies:example::[22:14] <Bob> yes, after lunch'
        --expect-room-view '!replies:example::    > <Bob> yes, after lunch'
        --expect-room-view '!replies:example::[22:15] * Carol cheers'
        --expect-room-view '!replies:example::[22:16] <Dave> I will tag the release'
        --expect-room-view '!replies:example::[22:17] <Alice> the deploy window is 14:00 UTC (edited)'
        --expect-room-view '!replies:example::    > <Dave> the login page times out on the second try'
        --expect-room-view '!replies:example::    > (loading the replied-to message...)'
        --expect-room-view '!replies:example::* Replying to <Alice> are we deploying today? the build looks green on eve... (Esc cancels)'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!replies:example'
      ;;
    line-marker)
      # The read marker names Carol's reacted line: the rule sits below its
      # reactions, above Dave's line; the echo line goes last.
      expect_selected='!marker:example'
      expect_active_view='room:!marker:example'
      ordered_actions=('input:/reply $marker-question')
      room_expect_args=(
        --expect-room '!marker:example'
      )
      room_view_expect_args=(
        --expect-room-view '!marker:example::    > <Alice> are we deploying today?'
        --expect-room-view '!marker:example::<Carol> the build is green on every platform'
        --expect-room-view '!marker:example::reactions: +1 Alice, Dave'
        --expect-room-view '!marker:example::<Dave> I will tag the release'
        --expect-room-view '!marker:example::* Replying to <Alice> are we deploying today? (Esc cancels)'
      )
      expect_switchbar_active='room:!marker:example'
      ;;
    line-marker-unshown)
      # The read marker names an event no line shows (an edit of Dave's
      # line), sent between Dave's line and Alice's: the rule sits below
      # Dave's line, by the marker's time.
      expect_selected='!marker:example'
      expect_active_view='room:!marker:example'
      room_expect_args=(
        --expect-room '!marker:example'
      )
      room_view_expect_args=(
        --expect-room-view '!marker:example::<Dave> I will tag the release'
        --expect-room-view '!marker:example::<Bob> thanks'
      )
      expect_switchbar_active='room:!marker:example'
      ;;
    redactions)
      expect_selected='!redactions:example'
      expect_active_view='room:!redactions:example'
      append_events_path="$root_dir/tests/ui-state-updates/redactions.json"
      select_views=('thread:!redactions:example|$redact-thread-root' 'room:!redactions:example')
      room_expect_args=(
        --expect-room '!redactions:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!redactions:example::"type":"redacted","event_id":"$redact-text"'
        --expect-room-activity '!redactions:example::"type":"redacted","event_id":"$redact-edited"'
        --expect-room-activity '!redactions:example::"type":"redacted","event_id":"$redact-image"'
        --expect-room-activity '!redactions:example::"type":"redacted","event_id":"$redact-unsupported"'
        --expect-room-activity '!redactions:example::"type":"redacted","event_id":"$redact-thread-root"'
        --expect-room-activity '!redactions:example::"type":"redacted","event_id":"$redact-thread-reply"'
        --expect-room-activity '!redactions:example::"type":"redacted","event_id":"$redact-topic-notice"'
        --expect-room-activity '!redactions:example::"type":"thread"'
      )
      room_view_expect_args=(
        --expect-room-view '!redactions:example::* Now talking in Redactions (!redactions:example)'
        --expect-room-view '!redactions:example::message redacted'
      )
      view_expect_args=(
        --expect-view 'thread:!redactions:example|$redact-thread-root::message redacted'
      )
      view_forbid_args=(
        --expect-no-view 'room:!redactions:example::redaction text payload should disappear'
        --expect-no-view 'room:!redactions:example::redaction edited original should disappear'
        --expect-no-view 'room:!redactions:example::redaction edited replacement should disappear'
        --expect-no-view 'room:!redactions:example::redaction-image-body.png'
        --expect-no-view 'room:!redactions:example::redaction-image-file.png'
        --expect-no-view 'room:!redactions:example::mxc://example.org/redaction-image-source'
        --expect-no-view 'room:!redactions:example::loading image...'
        --expect-no-view 'room:!redactions:example::image unavailable'
        --expect-no-view 'room:!redactions:example::redaction unsupported payload should disappear'
        --expect-no-view 'room:!redactions:example::redaction thread root payload should disappear'
        --expect-no-view 'room:!redactions:example::redaction thread reply payload should disappear'
        --expect-no-view "room:!redactions:example::@grace:example changes topic to 'redaction topic should disappear'"
        --expect-no-view 'thread:!redactions:example|$redact-thread-root::redaction thread root payload should disappear'
        --expect-no-view 'thread:!redactions:example|$redact-thread-root::redaction thread reply payload should disappear'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!redactions:example::Redactions'
        --expect-switchbar-item 'thread:!redactions:example|$redact-thread-root::Message redacted'
      )
      expect_switchbar_active='room:!redactions:example'
      navigation_expect_args=(
        --expect-navigation-text 'Redactions'
        --expect-navigation-text 'Message redacted'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text 'redaction thread root payload should disappear'
      )
      ;;
    verification-error)
      expect_selected='!verify:example'
      expect_active_view='room:!verify:example'
      expect_verification='flow-verify-123'
      expect_error='message send failed'
      room_expect_args=(
        --expect-room '!support:example'
        --expect-room '!verify:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!verify:example::@alice:example'
        --expect-room-activity '!verify:example::verification requested'
        --expect-room-activity '!verify:example::sas emoji ready'
        --expect-room-activity '!verify:example::waiting for confirmation'
      )
      room_view_expect_args=(
        --expect-room-view '!verify:example::* Now talking in Verify (!verify:example)'
        --expect-room-view '!verify:example::[22:14] <@alice:example> verification requested by @alice:example'
        --expect-room-view '!verify:example::[22:15] <@alice:example> sas emoji ready'
        --expect-room-view '!verify:example::[22:16] <@alice:example> waiting for confirmation'
      )
      status_view_expect_args=(
        --expect-status-view 'flow-verify-123'
        --expect-status-view 'message send failed'
        --expect-status-view '* Actions: Tools > Accept Verification, Tools > Cancel Verification, /verify accept, /verify cancel'
      )
      status_bar_expect_args=(
        --expect-status-bar 'error during send text'
        --expect-status-bar 'flow-verify-123'
        --expect-status-bar 'message send failed'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!verify:example::Verify'
        --expect-switchbar-item 'room:!support:example::Support'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!support:example::message'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!verify:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!verify:example'
      navigation_forbid_args=(
        --expect-no-navigation-text 'verification requested'
        --expect-no-navigation-text 'sas emoji ready'
        --expect-no-navigation-text 'waiting for confirmation'
        --expect-no-navigation-text 'flow-verify-123'
        --expect-no-navigation-text 'message send failed'
      )
      ;;
    verification-request)
      expect_active_view='status'
      expect_verification='flow-verify-request'
      expect_verification_state='requested'
      verification_text_expect_args=(
        --expect-verification-text 'flow_id=flow-verify-request'
        --expect-verification-text 'user_id=@alice:example'
        --expect-verification-text 'state=requested'
      )
      status_view_expect_args=(
        --expect-status-view '[22:12] * Verification requested by @alice:example (flow flow-verify-request)'
        --expect-status-view '* Actions: Tools > Accept Verification, Tools > Cancel Verification, /verify accept, /verify cancel'
      )
      status_bar_expect_args=(
        --expect-status-bar 'verification requested by @alice:example'
        --expect-status-bar 'verification: requested flow-verify-request from @alice:example'
      )
      status_view_forbid_args=(
        --expect-no-status-view '/verify confirm'
        --expect-no-status-view '/verify reject'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='status'
      navigation_forbid_args=(
        --expect-no-navigation-text 'flow-verify-request'
        --expect-no-navigation-text '@alice:example'
      )
      ;;
    verification-sent)
      expect_active_view='status'
      expect_verification='flow-verify-sent'
      expect_verification_state='sent'
      verification_text_expect_args=(
        --expect-verification-text 'flow_id=flow-verify-sent'
        --expect-verification-text 'user_id=@alice:example'
        --expect-verification-text 'state=sent'
      )
      status_view_expect_args=(
        --expect-status-view '[22:12] * Verification request sent to @alice:example; waiting for them to accept'
        --expect-status-view '* Actions: Tools > Cancel Verification, /verify cancel'
      )
      status_bar_expect_args=(
        --expect-status-bar 'verification request sent to @alice:example'
        --expect-status-bar 'verification: sent flow-verify-sent to @alice:example'
      )
      status_view_forbid_args=(
        --expect-no-status-view '/verify accept'
        --expect-no-status-view '/verify confirm'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='status'
      ;;
    verification-sas-ready)
      expect_active_view='status'
      expect_verification='flow-verify-sas-ready'
      expect_verification_state='sas_ready'
      verification_text_expect_args=(
        --expect-verification-text 'flow_id=flow-verify-sas-ready'
        --expect-verification-text 'state=sas_ready'
        --expect-verification-text 'emoji=🐶 dog | 🐱 cat | 🦁 lion | 🐎 horse | 🦄 unicorn | 🐷 pig | 🐘 elephant'
        --expect-verification-text 'decimals=1234-5678-9012'
      )
      status_view_expect_args=(
        --expect-status-view '[22:12] * Verification with @alice:example: compare the emoji or the numbers'
        --expect-status-view '* Emoji: 🐶 dog | 🐱 cat | 🦁 lion | 🐎 horse | 🦄 unicorn | 🐷 pig | 🐘 elephant'
        --expect-status-view '* Decimals: 1234-5678-9012'
        --expect-status-view '* Actions: Tools > Confirm SAS, Tools > Reject SAS, Tools > Cancel Verification, /verify confirm, /verify reject, /verify cancel'
      )
      status_bar_expect_args=(
        --expect-status-bar 'verification sas ready with @alice:example'
        --expect-status-bar 'verification: sas ready flow-verify-sas-ready from @alice:example'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='status'
      navigation_forbid_args=(
        --expect-no-navigation-text 'flow-verify-sas-ready'
        --expect-no-navigation-text '1234-5678-9012'
      )
      ;;
    verification-done)
      expect_active_view='status'
      expect_verification='flow-verify-done'
      expect_verification_state='done'
      verification_text_expect_args=(
        --expect-verification-text 'flow_id=flow-verify-done'
        --expect-verification-text 'state=done'
      )
      status_view_expect_args=(
        --expect-status-view '[22:12] * Verification done with @alice:example: verified'
      )
      status_view_forbid_args=(
        --expect-no-status-view '/verify confirm'
        --expect-no-status-view '/verify reject'
        --expect-no-status-view '/verify cancel'
      )
      status_bar_expect_args=(
        --expect-status-bar 'verification done with @alice:example'
        --expect-status-bar 'verification: done flow-verify-done from @alice:example'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='status'
      navigation_forbid_args=(
        --expect-no-navigation-text 'flow-verify-done'
        --expect-no-navigation-text 'verified'
      )
      ;;
    verification-cancelled)
      expect_active_view='status'
      expect_verification='flow-verify-cancelled'
      expect_verification_state='cancelled'
      verification_text_expect_args=(
        --expect-verification-text 'flow_id=flow-verify-cancelled'
        --expect-verification-text 'state=cancelled'
        --expect-verification-text 'reason=timed out waiting for confirmation'
        --expect-verification-text 'code=m.timeout'
      )
      status_view_expect_args=(
        --expect-status-view '[22:12] * Verification cancelled with @alice:example: timed out waiting for confirmation (m.timeout)'
      )
      status_view_forbid_args=(
        --expect-no-status-view '/verify confirm'
        --expect-no-status-view '/verify reject'
        --expect-no-status-view '/verify cancel'
      )
      status_bar_expect_args=(
        --expect-status-bar 'verification cancelled with @alice:example'
        --expect-status-bar 'verification: cancelled flow-verify-cancelled from @alice:example'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='status'
      navigation_forbid_args=(
        --expect-no-navigation-text 'flow-verify-cancelled'
        --expect-no-navigation-text 'm.timeout'
      )
      ;;
    e2ee-recovery-needed)
      expect_active_view='status'
      status_view_expect_args=(
        --expect-status-view '[22:12] * E2EE: recovery incomplete, local backup disabled, server backup exists, cross-signing incomplete, this session not verified'
        --expect-status-view '[22:12] * This session is not verified: /verify to verify it from another of your sessions, /e2ee recover with your recovery key, or /e2ee reset if you have neither'
        --expect-status-view '* Use /e2ee recover to restore keys'
      )
      status_view_forbid_args=(
        --expect-no-status-view 'recovery_secret'
        --expect-no-status-view 'passphrase='
      )
      status_bar_expect_args=(
        --expect-status-bar 'e2ee: recovery incomplete, local backup disabled, cross-signing incomplete, session not verified'
      )
      status_bar_forbid_args=(
        --expect-no-status-bar 'recovery_secret'
        --expect-no-status-bar 'passphrase='
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='status'
      ;;
    e2ee-recovery-enabled)
      expect_active_view='status'
      status_view_expect_args=(
        --expect-status-view '[22:12] * E2EE: recovery enabled, local backup enabled, server backup exists, cross-signing complete, this session verified'
      )
      status_view_forbid_args=(
        --expect-no-status-view '* Use /e2ee'
        --expect-no-status-view 'recovery_secret'
        --expect-no-status-view 'passphrase='
      )
      status_bar_expect_args=(
        --expect-status-bar 'e2ee: recovery enabled, backup healthy, cross-signing complete'
      )
      status_bar_forbid_args=(
        --expect-no-status-bar 'recovery_secret'
        --expect-no-status-bar 'passphrase='
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='status'
      ;;
    e2ee-recovery-error)
      expect_error='recover_e2ee: failed to recover E2EE secrets'
      expect_active_view='status'
      status_view_expect_args=(
        --expect-status-view '[22:12] * E2EE: recovery incomplete, local backup disabled, server backup exists, cross-signing incomplete, this session not verified'
        --expect-status-view '[22:12] * This session is not verified: /verify to verify it from another of your sessions, /e2ee recover with your recovery key, or /e2ee reset if you have neither'
        --expect-status-view '* Use /e2ee recover to restore keys'
        --expect-status-view '[22:12] * Error during e2ee recovery: failed to recover E2EE secrets: invalid recovery key or passphrase'
      )
      status_view_forbid_args=(
        --expect-no-status-view 'recovery_secret'
        --expect-no-status-view 'passphrase='
      )
      status_bar_expect_args=(
        --expect-status-bar 'e2ee: recovery incomplete, local backup disabled, cross-signing incomplete, session not verified'
        --expect-status-bar 'error: e2ee recovery: failed to recover E2EE secrets'
      )
      status_bar_forbid_args=(
        --expect-no-status-bar 'recovery_secret'
        --expect-no-status-bar 'passphrase='
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='status'
      ;;
    e2ee-backup-missing)
      expect_active_view='status'
      status_view_expect_args=(
        --expect-status-view '[22:12] * E2EE: recovery enabled, local backup enabled, server backup missing, cross-signing complete, this session verified'
      )
      status_view_forbid_args=(
        --expect-no-status-view '* Use /e2ee'
        --expect-no-status-view 'recovery_secret'
        --expect-no-status-view 'passphrase='
      )
      status_bar_expect_args=(
        --expect-status-bar 'e2ee: recovery enabled, backup missing on server, cross-signing complete'
      )
      status_bar_forbid_args=(
        --expect-no-status-bar 'recovery_secret'
        --expect-no-status-bar 'passphrase='
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='status'
      ;;
    e2ee-identity-reset-approval)
      # The server confirms a reset in the browser: mirx opened the page.
      expect_active_view='status'
      status_view_expect_args=(
        --expect-status-view '[22:12] * To reset your E2EE identity, approve it on the page your browser opened; mirx waits 2 minutes, /e2ee reset cancel stops it'
        --expect-status-view '[22:12] * If no browser opened, go to https://account.example/account?action=org.matrix.cross_signing_reset'
      )
      status_bar_expect_args=(
        --expect-status-bar 'waiting for identity reset approval'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='status'
      ;;
    e2ee-identity-reset-done)
      # The reset uploaded a new identity that signs this session; the
      # backup came back, recovery did not.
      expect_active_view='status'
      status_view_expect_args=(
        --expect-status-view '[22:12] * Your E2EE identity was reset; this session is verified with the new one'
        --expect-status-view '[22:12] * Recovery is off: /e2ee setup to set it up again'
        --expect-status-view ', this session verified'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='status'
      ;;
    e2ee-session-unverified)
      # Your identity is set up elsewhere, without recovery: this session
      # holds no keys and needs verifying.
      expect_active_view='status'
      status_view_expect_args=(
        --expect-status-view '[22:12] * E2EE: recovery disabled, local backup unknown, server backup missing, cross-signing incomplete, this session not verified'
        --expect-status-view '[22:12] * This session is not verified: /verify to verify it from another of your sessions, /e2ee recover with your recovery key, or /e2ee reset if you have neither'
      )
      status_bar_expect_args=(
        --expect-status-bar ', session not verified'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='status'
      ;;
    mixed-room-activity)
      expect_selected='!mixed:example'
      expect_active_view='room:!mixed:example'
      room_expect_args=(
        --expect-room '!mixed:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!mixed:example::synced hello'
        --expect-room-activity '!mixed:example::server maintenance'
        --expect-room-activity '!mixed:example::waves hello'
        --expect-room-activity '!mixed:example::pending local echo'
        --expect-room-activity '!mixed:example::failed local echo'
        --expect-room-activity '!mixed:example::mixed room topic'
        --expect-room-activity '!mixed:example::join'
        --expect-room-activity '!mixed:example::leave'
        --expect-room-activity '!mixed:example::"state":"failed"'
      )
      room_view_expect_args=(
        --expect-room-view '!mixed:example::* Now talking in Mixed (!mixed:example)'
        --expect-room-view "!mixed:example::* Topic is 'mixed room topic'"
        --expect-room-view '!mixed:example::[22:13] <Alice> synced hello'
        --expect-room-view "!mixed:example::[22:14] * Alice changes topic to 'mixed room topic'"
        --expect-room-view '!mixed:example::[22:15] * Joins: Bob (@bob:example)'
        --expect-room-view '!mixed:example::[22:16] * Parts: carol (@carol:example)'
        --expect-room-view '!mixed:example::[22:17] -Server- server maintenance'
        --expect-room-view '!mixed:example::[22:18] * Alice waves hello'
        --expect-room-view '!mixed:example::[22:17] * Alice pending local echo (sending)'
        --expect-room-view '!mixed:example::[22:18] * Alice failed local echo (failed)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!mixed:example::Mixed'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!mixed:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!mixed:example'
      navigation_forbid_args=(
        --expect-no-navigation-text 'synced hello'
        --expect-no-navigation-text 'pending local echo'
        --expect-no-navigation-text 'failed local echo'
      )
      ;;
    treebar-resized)
      expect_selected='!dense:example'
      expect_active_view='room:!dense:example'
      # The dense fixture with the treebar's splitter dragged right: the
      # long thread label shows whole, without a horizontal scrollbar.
      ordered_actions=(drag-treebar-splitter:280)
      room_expect_args=(
        --expect-room '!dense:example'
      )
      room_view_expect_args=(
        --expect-room-view '!dense:example::* Now talking in Dense (!dense:example)'
      )
      ;;
    dense-timeline-log)
      expect_selected='!dense:example'
      expect_active_view='room:!dense:example'
      room_expect_args=(
        --expect-room '!dense:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!dense:example::@dense.sender.with.a.very.long.mxid:example.org'
        --expect-room-activity '!dense:example::dense text row alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
        --expect-room-activity '!dense:example::dense notice row alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
        --expect-room-activity '!dense:example::dense emote row alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
        --expect-room-activity '!dense:example::dense topic alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
        --expect-room-activity '!dense:example::Dense Member Alpha Beta Gamma Delta'
        --expect-room-activity '!dense:example::@dense-leaver:example'
        --expect-room-activity '!dense:example::"type":"audio"'
        --expect-room-activity '!dense:example::voice-note.ogg'
        --expect-room-activity '!dense:example::audio/ogg'
        --expect-room-activity '!dense:example::23456'
        --expect-room-activity '!dense:example::12000'
        --expect-room-activity '!dense:example::unsupported_message'
        --expect-room-activity '!dense:example::com.example.custom'
        --expect-room-activity '!dense:example::dense custom payload alpha beta gamma delta epsilon zeta'
        --expect-room-activity '!dense:example::dense pending row alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
        --expect-room-activity '!dense:example::dense failed row alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
        --expect-room-activity '!dense:example::"state":"failed"'
        --expect-room-activity '!dense:example::dense-diagram-alpha-beta-gamma-delta.png'
        --expect-room-activity '!dense:example::"type":"thread"'
        --expect-room-activity '!dense:example::dense thread root alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
        --expect-room-activity '!dense:example::dense threaded reply alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
      )
      room_view_expect_args=(
        --expect-room-view '!dense:example::* Now talking in Dense (!dense:example)'
        --expect-room-view "!dense:example::* Topic is 'dense topic alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'"
        --expect-room-view '!dense:example::[22:13] <@dense.sender.with.a.very.long.mxid:example.org> dense text row alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
        --expect-room-view '!dense:example::[22:14] -Server- dense notice row alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
        --expect-room-view '!dense:example::[22:15] * Alice dense emote row alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
        --expect-room-view "!dense:example::[22:16] * Alice changes topic to 'dense topic alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'"
        --expect-room-view '!dense:example::[22:17] * Joins: Dense Member Alpha Beta Gamma Delta (@dense-member:example)'
        --expect-room-view '!dense:example::[22:17] * Parts: dense-leaver (@dense-leaver:example)'
        --expect-room-view '!dense:example::[22:18] * Alice sent audio: voice-note.ogg (audio/ogg, 23456 bytes, 12000 ms)'
        --expect-room-view '!dense:example::[22:18] * Alice sent unsupported com.example.custom: dense custom payload alpha beta gamma delta epsilon zeta'
        --expect-room-view '!dense:example::[22:19] <Alice> dense pending row alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu (sending)'
        --expect-room-view '!dense:example::[22:20] <Alice> dense failed row alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu (failed)'
        --expect-room-view '!dense:example::[22:21] * Alice sent image: dense-diagram-alpha-beta-gamma-delta.png (image/png, 640x480, 12345 bytes)'
        --expect-room-view '!dense:example::[22:22] <Bob> dense thread root alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
        --expect-room-view '!dense:example::[22:23] <Carol> dense threaded reply alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!dense:example::Dense'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!dense:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!dense:example'
      navigation_forbid_args=(
        --expect-no-navigation-text 'dense text row alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
        --expect-no-navigation-text 'dense failed row alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
        --expect-no-navigation-text 'dense threaded reply alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu'
      )
      ;;
    history-backfill)
      expect_selected='!history:example'
      expect_active_view='room:!history:example'
      append_events_path="$root_dir/tests/ui-state-updates/history-backfill.json"
      room_expect_args=(
        --expect-room '!history:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!history:example::older room line one'
        --expect-room-activity '!history:example::older room line two'
        --expect-room-activity '!history:example::current room line one'
        --expect-room-activity '!history:example::current room line two'
      )
      room_view_expect_args=(
        --expect-room-view '!history:example::* Now talking in History (!history:example)'
        --expect-room-view '!history:example::older room line one'
        --expect-room-view '!history:example::older room line two'
        --expect-room-view '!history:example::current room line one'
        --expect-room-view '!history:example::current room line two'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!history:example::History'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!history:example'
        --expect-no-switchbar-activity 'status'
      )
      navigation_expect_args=(
        --expect-navigation-text 'History'
      )
      navigation_no_activity_expect_args=(
        --expect-no-navigation-activity 'room:!history:example'
        --expect-no-navigation-activity 'status'
      )
      expect_switchbar_active='room:!history:example'
      ;;
    history-backfill-loading)
      expect_selected='!history:example'
      expect_active_view='room:!history:example'
      trigger_history_backfill=1
      room_expect_args=(
        --expect-room '!history:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!history:example::current room line one'
        --expect-room-activity '!history:example::current room line two'
      )
      room_view_expect_args=(
        --expect-room-view '!history:example::* Now talking in History (!history:example)'
        --expect-room-view '!history:example::* Loading older history...'
        --expect-room-view '!history:example::current room line one'
        --expect-room-view '!history:example::current room line two'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!history:example::History'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!history:example'
        --expect-no-switchbar-activity 'status'
      )
      navigation_expect_args=(
        --expect-navigation-text 'History'
      )
      navigation_no_activity_expect_args=(
        --expect-no-navigation-activity 'room:!history:example'
        --expect-no-navigation-activity 'status'
      )
      expect_switchbar_active='room:!history:example'
      ;;
    history-backfill-failed)
      expect_selected='!history:example'
      expect_active_view='room:!history:example'
      trigger_history_backfill=1
      append_events_path="$root_dir/tests/ui-state-updates/history-backfill-failed.json"
      expect_error='homeserver unavailable'
      room_expect_args=(
        --expect-room '!history:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!history:example::current room line one'
        --expect-room-activity '!history:example::current room line two'
      )
      room_view_expect_args=(
        --expect-room-view '!history:example::* Now talking in History (!history:example)'
        --expect-room-view '!history:example::* Unable to load older history: homeserver unavailable. Scroll to top to retry.'
        --expect-room-view '!history:example::current room line one'
        --expect-room-view '!history:example::current room line two'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!history:example::History'
      )
      # The failure is an error line in the inactive Status window.
      status_view_expect_args=(
        --expect-status-view '* Error during backfill room history: homeserver unavailable'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'status::message'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!history:example'
      )
      navigation_expect_args=(
        --expect-navigation-text 'History'
      )
      navigation_activity_expect_args=(
        --expect-navigation-activity 'status::message'
      )
      navigation_no_activity_expect_args=(
        --expect-no-navigation-activity 'room:!history:example'
      )
      expect_switchbar_active='room:!history:example'
      ;;
    history-backfill-exhausted)
      expect_selected='!history:example'
      expect_active_view='room:!history:example'
      trigger_history_backfill=1
      append_events_path="$root_dir/tests/ui-state-updates/history-backfill-exhausted.json"
      room_expect_args=(
        --expect-room '!history:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!history:example::current room line one'
        --expect-room-activity '!history:example::current room line two'
      )
      room_view_expect_args=(
        --expect-room-view '!history:example::* Now talking in History (!history:example)'
        --expect-room-view '!history:example::* No older history available.'
        --expect-room-view '!history:example::current room line one'
        --expect-room-view '!history:example::current room line two'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!history:example::History'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!history:example'
        --expect-no-switchbar-activity 'status'
      )
      navigation_expect_args=(
        --expect-navigation-text 'History'
      )
      navigation_no_activity_expect_args=(
        --expect-no-navigation-activity 'room:!history:example'
        --expect-no-navigation-activity 'status'
      )
      expect_switchbar_active='room:!history:example'
      ;;
    member-list)
      expect_selected='!members:example'
      expect_active_view='room:!members:example'
      room_expect_args=(
        --expect-room '!members:example'
      )
      room_view_expect_args=(
        --expect-room-view '!members:example::* Now talking in Members (!members:example)'
        --expect-room-view '!members:example::[22:13] <Alice> member list context line'
        --expect-room-view '!members:example::[22:14] <Casey> timeline stays visible beside the list'
        --expect-room-view '!members:example::[22:15] <Sam (@sam-one:example)> shared names show the user ID'
      )
      room_member_expect_args=(
        --expect-room-member '!members:example::Alice'
        --expect-room-member '!members:example::Sam (@sam-one:example)'
        --expect-room-member '!members:example::@ghost:example'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!members:example::Members'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!members:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!members:example'
      navigation_forbid_args=(
        --expect-no-navigation-text 'member list context line'
      )
      ;;
    glyph-fallback)
      expect_selected='!unicode:example'
      expect_active_view='room:!unicode:example'
      room_expect_args=(
        --expect-room '!unicode:example'
      )
      room_view_expect_args=(
        --expect-room-view '!unicode:example::* Now talking in Unicode (!unicode:example)'
        --expect-room-view '!unicode:example::[22:13] <Zoë 🌸> it’s ready — ship it 🚀'
        --expect-room-view '!unicode:example::reactions: 👍 Ωmega, Zoë 🌸'
        --expect-room-view '!unicode:example::[22:14] <Ωmega> naïve café ½'
        --expect-room-view '!unicode:example::[22:15] <Zoë 🌸> Привет, мир / ελληνικά'
        --expect-room-view $'!unicode:example::[22:16] <Ωmega> 👍\uFE0F with VS16, 👨\u200D👩\u200D👧 family, 🇩🇪 flag'
        --expect-room-view '!unicode:example::[22:17] <Zoë 🌸> 日本語'
      )
      room_member_expect_args=(
        --expect-room-member '!unicode:example::Zoë 🌸'
        --expect-room-member '!unicode:example::Ωmega'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!unicode:example::Unicode'
      )
      expect_switchbar_active='room:!unicode:example'
      ;;
    rich-text)
      expect_selected='!rich:example'
      expect_active_view='room:!rich:example'
      room_expect_args=(
        --expect-room '!rich:example'
      )
      room_view_expect_args=(
        --expect-room-view '!rich:example::[22:14] <Alice> bold italic курсив underline strike all'
        --expect-room-view '!rich:example::[22:15] <Bob> 00 01 02 03 04 05 06 07 08 09 10 11 12 13 14 15 0,1 8,2'
        --expect-room-view '!rich:example::[22:16] <Carol> extended colour 61 violet'
        --expect-room-view '!rich:example::[22:17] <Bob> Alice: the green build is yours'
        --expect-room-view '!rich:example::[22:18] * Carol waves hard'
        --expect-room-view '!rich:example::[22:19] -Bob- deploy window starts now'
        --expect-room-view '!rich:example::carries on past the break onto the next line before ending plain'
        --expect-room-view '!rich:example::[22:21] <Carol> todo:'
        --expect-room-view '!rich:example::* ship it'
        --expect-room-view '!rich:example::> quoted wisdom'
        --expect-room-view '!rich:example::let x = 1;'
        --expect-room-view '!rich:example::[22:22] <Bob> see docs (https://example.org/docs)'
        --expect-room-view '!rich:example::[22:23] <Alice> final text (edited)'
      )
      room_member_expect_args=(
        --expect-room-member '!rich:example::Alice'
        --expect-room-member '!rich:example::Bob'
        --expect-room-member '!rich:example::Carol'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!rich:example::Rich'
      )
      expect_switchbar_active='room:!rich:example'
      ;;
    hotlinks|hotlinks-selection)
      if [[ "$case_name" == hotlinks-selection ]]; then
        # Marks from the path of the wrapped address (visible line 8, after
        # "https://example.org/") into the next row, up to "C#" (line 10);
        # the button stays down, so the mark stays on screen.
        ordered_actions=(drag:8,20:10,18)
      fi
      expect_selected='!links:example'
      expect_active_view='room:!links:example'
      room_expect_args=(
        --expect-room '!links:example'
      )
      room_view_expect_args=(
        --expect-room-view '!links:example::<Alice> the spec is at https://spec.matrix.org/latest/ for anyone asking'
        --expect-room-view '!links:example::<Bob> mirror on www.example.org.'
        --expect-room-view '!links:example::<Carol> see docs (https://example.org/docs)'
        --expect-room-view '!links:example::<Alice> join https://matrix.to/#/%23rust%3Amatrix.org or #rust:matrix.org, ask https://matrix.to/#/@bob:example'
        --expect-room-view '!links:example::<Bob> logs: https://example.org/a/very/long/path/that/wraps/over/the/edge/of/the/window/to/the/next/line/index.html then more'
        --expect-room-view '!links:example::<Carol> C# is not a link, nor is issue#12'
        --expect-room-view '!links:example::<Bob> elsewhere: https://matrix.to/#/%23elsewhere%3Aexample.org?via=example.org'
        --expect-room-view '!links:example::<Carol> thanks Alice, and www.bob.example too'
      )
      room_member_expect_args=(
        --expect-room-member '!links:example::Alice'
        --expect-room-member '!links:example::Bob'
        --expect-room-member '!links:example::Carol'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!links:example::Links'
        --expect-switchbar-item 'room:!rust:example::Rust'
      )
      expect_switchbar_active='room:!links:example'
      ;;
    irc-commands)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      # Typed after the fixture's 22:10/22:11 lines, at the fixed 22:12
      # render-state clock, so the command lines print below them.
      # /whois's server answer is appended rather than typed: the snapshot
      # core is not signed in and would refuse it at no fixed moment.
      ordered_actions=(
        'input:/names'
        'input:/kick'
        'input:/kick Sam'
        "append:$root_dir/tests/ui-state-updates/irc-commands-whois.json"
        'input:/msg Alice hi there'
        'input:/help kick'
      )
      room_expect_args=(
        --expect-room '!ops:example'
        --expect-room '!dm-alice:example'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::[22:11] <Alice> who is around?'
        --expect-room-view '!ops:example::[22:12] * Names Ops: ~Creator &Admin @Mod +Voice Alan Alice Sam Sam'
        --expect-room-view '!ops:example::[22:12] * End of /NAMES list'
        --expect-room-view '!ops:example::[22:12] * /kick: insufficient parameters'
        --expect-room-view '!ops:example::[22:12] * /kick: Sam is ambiguous (@sam-one:example, @sam-two:example)'
        --expect-room-view '!ops:example::[22:12] * Alice is @alice:example * Alice Liddell'
        --expect-room-view '!ops:example::[22:12] * Alice on Ops Alice'
        --expect-room-view '!ops:example::[22:12] * Alice using example'
        --expect-room-view '!ops:example::[22:12] * Alice End of /WHOIS list'
        --expect-room-view '!ops:example::[22:12] -> *Alice* hi there'
        --expect-room-view '!ops:example::[22:12] * /kick <nick> [reason] - kick a member from this room'
      )
      # The status bar keeps the last command error.
      status_bar_expect_args=(
        --expect-status-bar '* /kick: Sam is ambiguous'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ops:example::Ops'
        --expect-switchbar-item 'room:!dm-alice:example::Alice'
      )
      expect_switchbar_active='room:!ops:example'
      ;;
    devices-list)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      # /devices' answer is appended rather than typed: the snapshot core
      # is not signed in and would refuse it at no fixed moment.
      ordered_actions=(
        "append:$root_dir/tests/ui-state-updates/devices-list.json"
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::[22:12] * Me sessions:'
        --expect-room-view '!ops:example::[22:12] * Me MIRXDEVICE "mirx" verified, last seen Fri Nov 20 14:02:11 2026 (this session)'
        --expect-room-view '!ops:example::[22:12] * Me PH0NE "Element Android" verified, last seen Thu Nov 19 14:02:11 2026'
        --expect-room-view '!ops:example::[22:12] * Me 0LDW3B "Element Web" unverified'
        --expect-room-view '!ops:example::[22:12] * Me End of /DEVICES list'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::@me:example'
        --expect-switchbar-item 'room:!ops:example::Ops'
      )
      expect_switchbar_active='room:!ops:example'
      ;;
    devices-signed-out)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      # The core's answers to /devices delete and /devices rename, appended:
      # the snapshot core is not signed in.
      ordered_actions=(
        "append:$root_dir/tests/ui-state-updates/devices-signed-out.json"
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::[22:12] * Session 0LDW3B signed out'
        --expect-room-view '!ops:example::[22:12] * Session PH0NE renamed "Work phone"'
      )
      status_bar_expect_args=(
        --expect-status-bar 'session renamed'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::@me:example'
        --expect-switchbar-item 'room:!ops:example::Ops'
      )
      expect_switchbar_active='room:!ops:example'
      ;;
    devices-sign-out-approval)
      # An OAuth 2.0 server signs sessions out on its account page; a
      # replayed load prints it, and opens nothing.
      expect_active_view='status'
      select_views=('status')
      status_view_expect_args=(
        --expect-status-view '[22:12] * To sign out PH0NE, confirm on the page your browser opened; /devices lists your sessions afterwards'
        --expect-status-view '[22:12] * If no browser opened, go to https://account.example/account?action=org.matrix.session_end&device_id=PH0NE'
      )
      status_view_forbid_args=(
        --expect-no-status-view 'Unable to open'
      )
      status_bar_expect_args=(
        --expect-status-bar 'waiting for session sign-out'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::@me:example'
        --expect-switchbar-item 'room:!ops:example::Ops'
      )
      expect_switchbar_active='status'
      ;;
    identity-warning)
      # Bob reset his identity, and so did Carol, whom you had verified;
      # /devices Bob then names the change.
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      ordered_actions=(
        "append:$root_dir/tests/ui-state-updates/identity-warning.json"
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::[22:12] * Warning: Bob'"'"'s identity was reset; /identity accept @bob:example to accept it, or /verify @bob:example'
        --expect-room-view '!ops:example::[22:12] * Warning: you verified Carol, and their identity was reset; /verify @carol:example to verify it again, or /identity accept @carol:example to withdraw your verification'
        --expect-room-view '!ops:example::[22:12] * Bob changed identity; /identity accept @bob:example, or /verify @bob:example'
        --expect-room-view '!ops:example::[22:12] * Bob B0BPH0NE "Bob'"'"'s phone" unverified'
        --expect-room-view '!ops:example::[22:12] * Bob End of /DEVICES list'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::@me:example'
        --expect-switchbar-item 'room:!ops:example::Ops'
      )
      expect_switchbar_active='room:!ops:example'
      ;;
    devices-user)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      ordered_actions=(
        "append:$root_dir/tests/ui-state-updates/devices-user.json"
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::[22:12] * Alice changed identity since you verified them; /verify @alice:example, or /identity accept @alice:example'
        --expect-room-view '!ops:example::[22:12] * Alice ALICEPHONE "Alice'"'"'s phone" unverified'
        --expect-room-view '!ops:example::[22:12] * Alice ALICELAPTOP unverified'
        --expect-room-view '!ops:example::[22:12] * Alice End of /DEVICES list'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::@me:example'
        --expect-switchbar-item 'room:!ops:example::Ops'
      )
      expect_switchbar_active='room:!ops:example'
      ;;
    irc-commands-who)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      # Rooms named by alias, the ignore list and a server's answers, at
      # the fixed 22:12 clock. The server's answers are appended: the
      # snapshot core is not signed in and would refuse a typed /ignore
      # <nick> or /whois at no fixed moment.
      ordered_actions=(
        'input:/topic'
        'input:/topic #hall'
        'input:/msg #lobby see ops'
        'input:/ignore'
        "append:$root_dir/tests/ui-state-updates/irc-commands-who.json"
        'input:/ignore'
      )
      room_expect_args=(
        --expect-room '!ops:example'
        --expect-room '!lobby:example'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::[22:12] * Topic is '"'"'incident review'"'"''
        --expect-room-view '!ops:example::[22:12] * No topic is set'
        --expect-room-view '!ops:example::[22:12] -> *#lobby* see ops'
        --expect-room-view '!ops:example::[22:12] * Ignore list is empty'
        --expect-room-view '!ops:example::[22:12] * Added Spam (@spam:example) to ignore list'
        --expect-room-view '!ops:example::[22:12] * Bob is @bob:example * Bob Builder'
        --expect-room-view '!ops:example::[22:12] * Bob on Ops Lobby'
        --expect-room-view '!ops:example::[22:12] * Bob using example'
        --expect-room-view '!ops:example::[22:12] * Bob End of /WHOIS list'
        --expect-room-view '!ops:example::[22:12] * Error during change room settings: you are not allowed to change power levels'
        --expect-room-view '!ops:example::[22:12] * Ignore list:'
        --expect-room-view '!ops:example::[22:12] *   Spam (@spam:example)'
      )
      # The appended error is the last thing to set the status.
      status_bar_expect_args=(
        --expect-status-bar 'error during change room settings'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ops:example::Ops'
        --expect-switchbar-item 'room:!lobby:example::Lobby'
      )
      expect_switchbar_active='room:!ops:example'
      ;;
    away-and-whois)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      # The server's answers to /away, /whois Bob and /back, appended at
      # the fixed 22:12 clock: the snapshot core is not signed in and
      # would refuse the typed commands at no fixed moment.
      ordered_actions=(
        "append:$root_dir/tests/ui-state-updates/away-and-whois.json"
      )
      room_expect_args=(
        --expect-room '!ops:example'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::[22:12] * You have been marked as being away'
        --expect-room-view '!ops:example::[22:12] * Others still see you as online: another of your sessions is active'
        --expect-room-view '!ops:example::[22:12] * Bob is @bob:example * Bob Builder'
        --expect-room-view '!ops:example::[22:12] * Bob on Ops'
        --expect-room-view '!ops:example::[22:12] * Bob using example'
        --expect-room-view '!ops:example::[22:12] * Bob is away: in a meeting'
        --expect-room-view '!ops:example::[22:12] * Bob has been idle 1hr 2mins'
        --expect-room-view '!ops:example::[22:12] * Bob End of /WHOIS list'
        --expect-room-view '!ops:example::[22:12] * You are no longer marked as being away'
      )
      # /back is the last thing to set the status.
      status_bar_expect_args=(
        --expect-status-bar 'back'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ops:example::Ops'
      )
      expect_switchbar_active='room:!ops:example'
      ;;
    aliases-and-echo)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      # Nothing here reaches the core, which is not signed in: /echo and
      # the alias's commands print locally, and /x stops at its bad second
      # command.
      ordered_actions=(
        'input:/echo plain echo'
        'input:/echo 4 -t red with a stamp'
        $'input:/echo \x02bold\x02 and \x034,1colour\x03 codes'
        'input:/alias x echo first $1 | bogus | echo never'
        'input:/x one'
        'input:/alias'
      )
      room_expect_args=(
        --expect-room '!ops:example'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::plain echo'
        --expect-room-view '!ops:example::[22:12] red with a stamp'
        --expect-room-view '!ops:example::bold and colour codes'
        --expect-room-view '!ops:example::first one'
        --expect-room-view '!ops:example::[22:12] * /bogus: unknown command (in alias /x)'
        --expect-room-view '!ops:example::[22:12] * Aliases:'
        --expect-room-view '!ops:example::[22:12] *   /j /join #$$1'
        --expect-room-view '!ops:example::[22:12] *   /x echo first $1 | bogus | echo never'
      )
      status_bar_expect_args=(
        --expect-status-bar '* /bogus: unknown command (in alias /x)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ops:example::Ops'
      )
      expect_switchbar_active='room:!ops:example'
      ;;
    room-list|room-list-listing)
      expect_active_view='room_list'
      # `/list` opens the Rooms List over the room; the directory's one
      # page fills it, or has not arrived yet.
      case "$case_name" in
        room-list)
          ordered_actions=(
            'input:/list'
            "append:$root_dir/tests/ui-state-updates/room-list-results.json"
          )
          mdi_title_expect_args=(
            --expect-mdi-title 'room_list::Rooms List [example] - 8 rooms'
          )
          ;;
        room-list-listing)
          ordered_actions=('input:/list')
          mdi_title_expect_args=(
            --expect-mdi-title 'room_list::Rooms List [example] - listing...'
          )
          ;;
      esac
      switchbar_item_expect_args=(
        --expect-switchbar-item 'room:!lobby:example::Lobby'
        --expect-switchbar-item 'room_list::Rooms List'
      )
      expect_switchbar_active='room_list'
      navigation_expect_args=(
        --expect-navigation-text 'Rooms List'
      )
      ;;
    timestamp-format)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      # The multi-room fixture with seconds in every stamp and the bitmap
      # Courier as the text font; the editbox, nick list and line height
      # follow the font.
      ordered_actions=(
        'input:/timestamp -f [HH:nn:ss]'
        'input:/font 12 Courier'
      )
      room_expect_args=(
        --expect-room '!lobby:example'
        --expect-room '!ops:example'
        --expect-room '!random:example'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::* Now talking in Ops (!ops:example)'
        --expect-room-view "!ops:example::[22:13:20] * Alice changes topic to 'incident review'"
        --expect-room-view '!ops:example::[22:14:20] * Joins: Carol (@carol:example)'
        --expect-room-view '!ops:example::[22:15:20] <Alice> ops topic: incident review'
      )
      status_bar_expect_args=(
        --expect-status-bar 'font: Courier 12'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ops:example::Ops'
      )
      expect_switchbar_active='room:!ops:example'
      ;;
    view-bars-hidden)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      # The multi-room fixture with all three bars hidden: the maximized
      # room fills the frame from the menubar to the status bar.
      ordered_actions=(
        'input:/toolbar off'
        'input:/switchbar off'
        'input:/treebar off'
      )
      room_expect_args=(
        --expect-room '!lobby:example'
        --expect-room '!ops:example'
        --expect-room '!random:example'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::* Now talking in Ops (!ops:example)'
        --expect-room-view "!ops:example::[22:13] * Alice changes topic to 'incident review'"
        --expect-room-view '!ops:example::[22:15] <Alice> ops topic: incident review'
      )
      status_bar_expect_args=(
        --expect-status-bar 'treebar: off'
      )
      frame_bar_hidden_expect_args=(
        --expect-frame-bar-hidden toolbar
        --expect-frame-bar-hidden switchbar
        --expect-frame-bar-hidden treebar
      )
      ;;
    away-nicks)
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      room_expect_args=(
        --expect-room '!ops:example'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::* Now talking in Ops (!ops:example)'
        --expect-room-view '!ops:example::[22:13] <Bob> back in an hour'
        --expect-room-view '!ops:example::[22:14] <Dave> the deploy is green'
        --expect-room-view '!ops:example::[22:15] <Alice> thanks, Dave'
      )
      room_member_expect_args=(
        --expect-room-member '!ops:example::Admin'
        --expect-room-member '!ops:example::Mod'
        --expect-room-member '!ops:example::Alice'
        --expect-room-member '!ops:example::Bob'
        --expect-room-member '!ops:example::Carol'
        --expect-room-member '!ops:example::Dave'
        --expect-room-member '!ops:example::Erin'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ops:example::Ops'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!ops:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!ops:example'
      navigation_forbid_args=(
        --expect-no-navigation-text 'back in an hour'
      )
      ;;
    nick-colors)
      # The away-nicks fixture with a line from Admin, then mIRC's nick
      # colours: Any mode in red (the staff), then anyone in an auto colour.
      # Away members stay grey in the nick list.
      ordered_actions=(
        'input:/cnick -a * 4'
        'input:/cnick -f * *'
      )
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      room_expect_args=(
        --expect-room '!ops:example'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::* Now talking in Ops (!ops:example)'
        --expect-room-view '!ops:example::[22:13] <Bob> back in an hour'
        --expect-room-view '!ops:example::[22:14] <Dave> the deploy is green'
        --expect-room-view '!ops:example::[22:15] <Alice> thanks, Dave'
        --expect-room-view '!ops:example::[22:16] <Admin> colours for the ops'
        --expect-room-view '!ops:example::[22:12] * Added * to nick color list'
      )
      room_member_expect_args=(
        --expect-room-member '!ops:example::Admin'
        --expect-room-member '!ops:example::Mod'
        --expect-room-member '!ops:example::Alice'
        --expect-room-member '!ops:example::Bob'
        --expect-room-member '!ops:example::Carol'
        --expect-room-member '!ops:example::Dave'
        --expect-room-member '!ops:example::Erin'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ops:example::Ops'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!ops:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!ops:example'
      ;;
    mode-prefix)
      # Options > IRC > Show mode prefix on: a channel's senders carry
      # their rank's glyph, members none.
      expect_selected='!ops:example'
      expect_active_view='room:!ops:example'
      ordered_actions=('options-check:6901=on')
      room_expect_args=(
        --expect-room '!ops:example'
      )
      room_view_expect_args=(
        --expect-room-view '!ops:example::[22:13] <~Owner> the room is mine'
        --expect-room-view '!ops:example::[22:14] * @Mod keeps the peace'
        --expect-room-view '!ops:example::[22:15] -+Voice- may speak when moderated'
        --expect-room-view '!ops:example::[22:16] <Alan> just a member'
      )
      ;;
    member-ranks)
      expect_selected='!ranks:example'
      expect_active_view='room:!ranks:example'
      room_expect_args=(
        --expect-room '!ranks:example'
      )
      room_view_expect_args=(
        --expect-room-view '!ranks:example::* Now talking in Ranks (!ranks:example)'
        --expect-room-view '!ranks:example::[22:13] <Creator> ranks sort above names'
        --expect-room-view '!ranks:example::[22:14] * Admin sets power level: Mod 0 -> 50, Voice 0 -> 10; default 10 -> 0'
        --expect-room-view '!ranks:example::[22:15] * Admin changes room permissions'
        --expect-room-view '!ranks:example::[22:16] <Sam (@sam-one:example)> shared names still show the user ID'
      )
      room_member_expect_args=(
        --expect-room-member '!ranks:example::Creator'
        --expect-room-member '!ranks:example::Admin'
        --expect-room-member '!ranks:example::Mod'
        --expect-room-member '!ranks:example::Voice'
        --expect-room-member '!ranks:example::@ghost:example'
        --expect-room-member '!ranks:example::Sam (@sam-two:example)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ranks:example::Ranks'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!ranks:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!ranks:example'
      navigation_forbid_args=(
        --expect-no-navigation-text 'ranks sort above names'
      )
      ;;
    settings-lines)
      # Power-level lines saying which settings they change (moderation
      # on; the topic to moderators only and moderation off; the topic to
      # everyone with other permissions), beside a levels-only line.
      expect_selected='!modes:example'
      expect_active_view='room:!modes:example'
      room_expect_args=(
        --expect-room '!modes:example'
      )
      room_view_expect_args=(
        --expect-room-view '!modes:example::* Now talking in Modes (!modes:example)'
        --expect-room-view '!modes:example::[22:13] * Admin turns moderation on; power level: Voice 0 -> 1'
        --expect-room-view '!modes:example::[22:14] * Admin turns moderation off and lets only moderators set the topic'
        --expect-room-view '!modes:example::[22:15] * Admin lets everyone set the topic; room permissions changed'
        --expect-room-view '!modes:example::[22:16] * Admin sets power level: Mod 0 -> 50'
      )
      room_member_expect_args=(
        --expect-room-member '!modes:example::Admin'
        --expect-room-member '!modes:example::Voice'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!modes:example::Modes'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!modes:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!modes:example'
      ;;
    member-list-narrow)
      expect_selected='!members:example'
      expect_active_view='room:!members:example'
      case_frame_size=640x768
      room_expect_args=(
        --expect-room '!members:example'
      )
      room_view_expect_args=(
        --expect-room-view '!members:example::* Now talking in Members (!members:example)'
        --expect-room-view '!members:example::[22:13] <Alice> member list context line'
        --expect-room-view '!members:example::[22:14] <Casey> timeline stays visible beside the list'
        --expect-room-view '!members:example::[22:15] <Sam (@sam-one:example)> shared names show the user ID'
      )
      room_member_expect_args=(
        --expect-room-member '!members:example::Alice'
        --expect-room-member '!members:example::Sam (@sam-one:example)'
        --expect-room-member '!members:example::@ghost:example'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!members:example::Members'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!members:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!members:example'
      navigation_forbid_args=(
        --expect-no-navigation-text 'member list context line'
      )
      ;;
    shell-activity)
      expect_selected='!alpha:example'
      expect_active_view='room:!alpha:example'
      select_views=('thread:!alpha:example|$alpha-thread-root' 'room:!alpha:example')
      append_events_path="$root_dir/tests/ui-state-updates/shell-activity.json"
      room_expect_args=(
        --expect-room '!alpha:example'
        --expect-room '!beta:example'
      )
      room_read_target_expect_args=(
        --expect-room-read-target '!alpha:example::null'
        --expect-room-read-target '!beta:example::$beta-activity'
      )
      room_view_expect_args=(
        --expect-room-view '!alpha:example::* Now talking in Alpha (!alpha:example)'
        --expect-room-view '!alpha:example::alpha parent room content'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!alpha:example::Alpha'
        --expect-switchbar-item 'room:!beta:example::Beta'
        --expect-switchbar-item 'thread:!alpha:example|$alpha-thread-root::alpha root'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!beta:example::message'
        --expect-switchbar-activity 'thread:!alpha:example|$alpha-thread-root::message'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
        --expect-no-switchbar-activity 'room:!alpha:example'
      )
      navigation_expect_args=(
        --expect-navigation-text 'Alpha'
        --expect-navigation-text 'Beta'
        --expect-navigation-text 'alpha root'
      )
      navigation_activity_expect_args=(
        --expect-navigation-activity 'room:!beta:example::message'
        --expect-navigation-activity 'thread:!alpha:example|$alpha-thread-root::message'
      )
      navigation_no_activity_expect_args=(
        --expect-no-navigation-activity 'status'
        --expect-no-navigation-activity 'room:!alpha:example'
      )
      expect_switchbar_active='room:!alpha:example'
      ;;
    activity-levels)
      expect_selected='!lobby:example'
      expect_active_view='room:!lobby:example'
      append_events_path="$root_dir/tests/ui-state-updates/activity-levels.json"
      room_view_expect_args=(
        --expect-room-view '!lobby:example::<Bob> Alice: ping'
        --expect-room-view '!lobby:example::* Bob waves at Alice'
      )
      room_activity_expect_args=(
        --expect-room-activity '!lobby:example::"highlight":true'
        --expect-room-activity '!pings:example::"highlight":true'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!events:example::event'
        --expect-switchbar-activity 'room:!chat:example::message'
        --expect-switchbar-activity 'room:!pings:example::highlight'
        --expect-switchbar-activity 'room:!casey:example::message'
        --expect-switchbar-activity 'room:!counts:example::highlight'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
        --expect-no-switchbar-activity 'room:!lobby:example'
      )
      navigation_activity_expect_args=(
        --expect-navigation-activity 'room:!events:example::event'
        --expect-navigation-activity 'room:!chat:example::message'
        --expect-navigation-activity 'room:!pings:example::highlight'
        --expect-navigation-activity 'room:!casey:example::message'
        --expect-navigation-activity 'room:!counts:example::highlight'
      )
      navigation_no_activity_expect_args=(
        --expect-no-navigation-activity 'status'
        --expect-no-navigation-activity 'room:!lobby:example'
      )
      expect_switchbar_active='room:!lobby:example'
      ;;
    notification-modes)
      expect_selected='!lobby:example'
      expect_active_view='room:!lobby:example'
      append_events_path="$root_dir/tests/ui-state-updates/notification-modes.json"
      room_activity_expect_args=(
        --expect-room-activity '!noisy:example::"highlight":true'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!busy:example::message'
        --expect-switchbar-activity 'room:!quiet:example::message'
        --expect-switchbar-activity 'room:!noisy:example::event'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
        --expect-no-switchbar-activity 'room:!lobby:example'
      )
      navigation_activity_expect_args=(
        --expect-navigation-activity 'room:!busy:example::message'
        --expect-navigation-activity 'room:!quiet:example::message'
        --expect-navigation-activity 'room:!noisy:example::event'
      )
      navigation_no_activity_expect_args=(
        --expect-no-navigation-activity 'status'
        --expect-no-navigation-activity 'room:!lobby:example'
      )
      expect_switchbar_active='room:!lobby:example'
      ;;
    shell-constrained-layout)
      expect_selected='!focus:example'
      expect_active_view='room:!focus:example'
      case_frame_size=900x620
      room_expect_args=(
        --expect-room '!focus:example'
        --expect-room '!inbox:example'
        --expect-room '!docs:example'
      )
      room_view_expect_args=(
        --expect-room-view '!focus:example::* Now talking in Focus (!focus:example)'
        --expect-room-view '!focus:example::long visible timeline line for the 900x620 shell frame so the MDI content remains obvious'
      )
      room_member_expect_args=(
        --expect-room-member '!focus:example::Alice'
        --expect-room-member '!focus:example::Casey'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 3 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!focus:example::Focus'
        --expect-switchbar-item 'room:!inbox:example::Inbox'
        --expect-switchbar-item 'room:!docs:example::Docs'
      )
      navigation_expect_args=(
        --expect-navigation-text 'Focus'
        --expect-navigation-text 'Inbox'
        --expect-navigation-text 'Docs'
      )
      expect_switchbar_active='room:!focus:example'
      ;;
    shell-many-open-views)
      expect_selected='!focus:example'
      expect_active_view='room:!focus:example'
      select_views=('thread:!focus:example|$focus-root' 'room:!focus:example')
      append_events_path="$root_dir/tests/ui-state-updates/shell-many-open-views.json"
      room_expect_args=(
        --expect-room '!alerts:example'
        --expect-room '!focus:example'
        --expect-room '!support:example'
        --expect-room '!query:example'
      )
      room_direct_expect_args=(
        --expect-direct-room '!query:example'
      )
      room_view_expect_args=(
        --expect-room-view '!focus:example::* Now talking in Focus (!focus:example)'
        --expect-room-view "!focus:example::* Topic is 'dense shell review'"
        --expect-room-view '!focus:example::active room remains readable among many switchbar views'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 10 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!focus:example::Focus'
        --expect-switchbar-item 'room:!alerts:example::Alerts'
        --expect-switchbar-item 'room:!support:example::Support Queue'
        --expect-switchbar-item 'room:!query:example::Casey'
        --expect-switchbar-item 'thread:!focus:example|$focus-root::review thread root'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!alerts:example::message'
        --expect-switchbar-activity 'room:!support:example::message'
        --expect-switchbar-activity 'thread:!focus:example|$focus-root::message'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
        --expect-no-switchbar-activity 'room:!focus:example'
      )
      navigation_expect_args=(
        --expect-navigation-text 'Alerts'
        --expect-navigation-text 'Focus'
        --expect-navigation-text 'Support Queue'
        --expect-navigation-text 'Casey'
        --expect-navigation-text 'review thread root'
      )
      navigation_activity_expect_args=(
        --expect-navigation-activity 'room:!alerts:example::message'
        --expect-navigation-activity 'room:!support:example::message'
        --expect-navigation-activity 'thread:!focus:example|$focus-root::message'
      )
      expect_switchbar_active='room:!focus:example'
      expect_switchbar_rows=2
      ;;
    shell-long-labels-constrained)
      expect_selected='!long-focus:example'
      expect_active_view='room:!long-focus:example'
      case_frame_size=900x620
      room_expect_args=(
        --expect-room '!long-focus:example'
        --expect-room '!long-inbox:example'
        --expect-room '!long-query:example'
      )
      room_direct_expect_args=(
        --expect-direct-room '!long-query:example'
      )
      room_view_expect_args=(
        --expect-room-view '!long-focus:example::* Now talking in Extremely Long Focus Room Name For Native Shell Layout Review (!long-focus:example)'
        --expect-room-view "!long-focus:example::* Topic is 'A deliberately long active-room topic"
        --expect-room-view '!long-focus:example::A deliberately long timeline message body checks'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 3 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!long-focus:example::Extremely Long Focus Room Name For Native Shell Layout Review'
        --expect-switchbar-item 'room:!long-inbox:example::Incoming Updates With An Intentionally Long Descriptive Room Label'
        --expect-switchbar-item 'room:!long-query:example::@very-long-direct-user-identifier-for-constrained-shell:example'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!long-inbox:example::message'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
        --expect-no-switchbar-activity 'room:!long-focus:example'
      )
      navigation_expect_args=(
        --expect-navigation-text 'Extremely Long Focus Room Name For Native Shell Layout Review'
        --expect-navigation-text 'Incoming Updates With An Intentionally Long Descriptive Room Label'
        --expect-navigation-text '@very-long-direct-user-identifier-for-constrained-shell:example'
      )
      expect_switchbar_active='room:!long-focus:example'
      ;;
    shell-activity-focus-constrained)
      expect_selected='!active:example'
      expect_active_view='room:!active:example'
      case_frame_size=900x620
      select_views=('thread:!active:example|$active-root' 'room:!active:example')
      append_events_path="$root_dir/tests/ui-state-updates/shell-activity-focus-constrained.json"
      room_expect_args=(
        --expect-room '!active:example'
        --expect-room '!inbox:example'
        --expect-room '!mentions:example'
      )
      room_view_expect_args=(
        --expect-room-view '!active:example::* Now talking in Active Focus (!active:example)'
        --expect-room-view '!active:example::active room keeps the compose and status shell visible'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 4 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!active:example::Active Focus'
        --expect-switchbar-item 'room:!inbox:example::Inbox'
        --expect-switchbar-item 'room:!mentions:example::Mentions'
        --expect-switchbar-item 'thread:!active:example|$active-root::active thread root'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!inbox:example::message'
        --expect-switchbar-activity 'room:!mentions:example::message'
        --expect-switchbar-activity 'thread:!active:example|$active-root::message'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'status'
        --expect-no-switchbar-activity 'room:!active:example'
      )
      navigation_expect_args=(
        --expect-navigation-text 'Active Focus'
        --expect-navigation-text 'Inbox'
        --expect-navigation-text 'Mentions'
        --expect-navigation-text 'active thread root'
      )
      navigation_activity_expect_args=(
        --expect-navigation-activity 'room:!inbox:example::message'
        --expect-navigation-activity 'room:!mentions:example::message'
        --expect-navigation-activity 'thread:!active:example|$active-root::message'
      )
      navigation_no_activity_expect_args=(
        --expect-no-navigation-activity 'status'
        --expect-no-navigation-activity 'room:!active:example'
      )
      expect_switchbar_active='room:!active:example'
      ;;
    # colors-dark: the membership lines and a nick list after a dark scheme
    # through the Colors dialog: every surface follows it.
    membership-changes|colors-dark)
      if [[ "$case_name" == colors-dark ]]; then
        ordered_actions=(
          'colors:background=1,normal=0,editbox=1,editbox_text=0,listbox=1,listbox_text=0,treebar=1,treebar_text=0,join=9,part=11,kick=4,settings=8,nick=13,topic=10,invite=7'
        )
      fi
      expect_selected='!membership:example'
      expect_active_view='room:!membership:example'
      room_expect_args=(
        --expect-room '!membership:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!membership:example::kicked_and_banned'
        --expect-room-activity '!membership:example::nick_changed'
        --expect-room-activity '!membership:example::flooding'
      )
      room_view_expect_args=(
        --expect-room-view '!membership:example::* Now talking in Membership (!membership:example)'
        --expect-room-view '!membership:example::[22:13] * Joins: Carol (@carol:example)'
        --expect-room-view '!membership:example::[22:14] * Parts: dave (@dave:example)'
        --expect-room-view '!membership:example::[22:15] * Eve was kicked by Op (flooding)'
        --expect-room-view '!membership:example::[22:16] * Frank was banned by Op (spam)'
        --expect-room-view '!membership:example::[22:17] * Grace was banned by Op'
        --expect-room-view '!membership:example::[22:18] * Op removed ban on Frank'
        --expect-room-view '!membership:example::[22:19] * Op invites Heidi'
        --expect-room-view '!membership:example::[22:20] * Joins: Heidi (@heidi:example)'
        --expect-room-view '!membership:example::[22:21] * Ivan declined the invitation'
        --expect-room-view '!membership:example::[22:22] * Op revoked the invitation for Judy'
        --expect-room-view '!membership:example::[22:23] * Mallory knocks (let me in)'
        --expect-room-view '!membership:example::[22:24] * Op accepted knock from Mallory'
        --expect-room-view '!membership:example::[22:25] * Niaj withdrew knock'
        --expect-room-view '!membership:example::[22:26] * Op denied knock from Olivia'
        --expect-room-view '!membership:example::[22:27] * Carol is now known as Caz'
        --expect-room-view '!membership:example::[22:28] * Sam (@sam:example) changed avatar'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!membership:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!membership:example'
      ;;
    nick-localparts)
      expect_selected='!nicks:example'
      expect_active_view='room:!nicks:example'
      room_expect_args=(
        --expect-room '!nicks:example'
      )
      room_view_expect_args=(
        --expect-room-view '!nicks:example::* Now talking in Nicks (!nicks:example)'
        --expect-room-view '!nicks:example::[22:13] * Joins: zed (@zed:example)'
        --expect-room-view '!nicks:example::[22:14] <ghost (@ghost:example)> boo'
        --expect-room-view '!nicks:example::[22:15] <mod> keep it civil'
        --expect-room-view '!nicks:example::    reactions: 👍 zed, mod'
        --expect-room-view '!nicks:example::[22:16] <@stranger:example> not a member'
        --expect-room-view '!nicks:example::* zed is typing'
      )
      room_member_expect_args=(
        --expect-room-member '!nicks:example::mod'
        --expect-room-member '!nicks:example::Ghost (@casper:example)'
        --expect-room-member '!nicks:example::ghost (@ghost:example)'
        --expect-room-member '!nicks:example::Me'
        --expect-room-member '!nicks:example::zed'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!nicks:example::Nicks'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!nicks:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!nicks:example'
      navigation_forbid_args=(
        --expect-no-navigation-text 'keep it civil'
      )
      ;;
    typing-indicator)
      expect_selected='!typing:example'
      expect_active_view='room:!typing:example'
      room_expect_args=(
        --expect-room '!typing:example'
      )
      room_view_expect_args=(
        --expect-room-view '!typing:example::* Now talking in Typing (!typing:example)'
        --expect-room-view '!typing:example::* @alice:example, @bob:example are typing'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!typing:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!typing:example'
      navigation_forbid_args=(
        --expect-no-navigation-text 'are typing'
      )
      ;;
    read-state-ingestion)
      expect_selected='!beta:example'
      expect_active_view='room:!beta:example'
      room_expect_args=(
        --expect-room '!beta:example'
        --expect-room '!alpha:example'
      )
      room_read_target_expect_args=(
        --expect-room-read-target '!alpha:example::null'
        --expect-room-read-target '!beta:example::null'
      )
      room_unread_expect_args=(
        --expect-room-unread '!alpha:example::1::2::1::true'
        --expect-room-unread '!beta:example::0::0::0::false'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!beta:example::Beta'
        --expect-switchbar-item 'room:!alpha:example::Alpha'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!alpha:example::highlight'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!beta:example'
        --expect-no-switchbar-activity 'status'
      )
      navigation_expect_args=(
        --expect-navigation-text 'Alpha'
      )
      expect_switchbar_active='room:!beta:example'
      ;;
    read-state-cleared)
      expect_selected='!beta:example'
      expect_active_view='room:!beta:example'
      append_events_path="$root_dir/tests/ui-state-updates/read-state-cleared.json"
      room_expect_args=(
        --expect-room '!beta:example'
        --expect-room '!alpha:example'
      )
      room_read_target_expect_args=(
        --expect-room-read-target '!alpha:example::null'
        --expect-room-read-target '!beta:example::null'
      )
      room_unread_expect_args=(
        --expect-room-unread '!alpha:example::0::0::0::false'
        --expect-room-unread '!beta:example::0::0::0::false'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!beta:example::Beta'
        --expect-switchbar-item 'room:!alpha:example::Alpha'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!alpha:example'
        --expect-no-switchbar-activity 'room:!beta:example'
        --expect-no-switchbar-activity 'status'
      )
      navigation_expect_args=(
        --expect-navigation-text 'Alpha'
      )
      expect_switchbar_active='room:!beta:example'
      ;;
    unsupported-message)
      expect_selected='!alpha:example.org'
      expect_active_view='room:!alpha:example.org'
      room_expect_args=(
        --expect-room '!alpha:example.org'
        --expect-room '!media:example.org'
      )
      room_activity_expect_args=(
        --expect-room-activity '!media:example.org::unsupported_message'
        --expect-room-activity '!media:example.org::com.example.custom'
        --expect-room-activity '!media:example.org::custom payload'
      )
      room_view_expect_args=(
        --expect-room-view '!media:example.org::* Now talking in Media (!media:example.org)'
        --expect-room-view '!media:example.org::[16:00] * @alice:example.org sent unsupported com.example.custom: custom payload'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 2 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!alpha:example.org::Alpha'
        --expect-switchbar-item 'room:!media:example.org::Media'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!media:example.org::message'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!alpha:example.org'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!alpha:example.org'
      view_forbid_args=(
        --expect-no-view 'room:!media:example.org::loading image...'
        --expect-no-view 'room:!media:example.org::image unavailable'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text 'custom payload'
      )
      ;;
    undecryptable-encrypted-message)
      expect_selected='!encrypted:example.org'
      expect_active_view='room:!encrypted:example.org'
      room_expect_args=(
        --expect-room '!alpha:example.org'
        --expect-room '!encrypted:example.org'
      )
      room_activity_expect_args=(
        --expect-room-activity '!encrypted:example.org::unable_to_decrypt'
        --expect-room-activity '!encrypted:example.org::$encrypted-utd1:example.org'
        --expect-room-activity '!encrypted:example.org::"cause":"unknown"'
        --expect-room-activity '!encrypted:example.org::"session_id":"SESSIONID"'
      )
      room_view_expect_args=(
        --expect-room-view '!encrypted:example.org::* Now talking in Encrypted (!encrypted:example.org)'
        --expect-room-view '!encrypted:example.org::[16:00] * @alice:example.org sent an encrypted message that could not be decrypted'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 2 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!alpha:example.org::Alpha'
        --expect-switchbar-item 'room:!encrypted:example.org::Encrypted'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!encrypted:example.org'
        --expect-no-switchbar-activity 'room:!alpha:example.org'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!encrypted:example.org'
      ;;
    # Each line that could not be decrypted says why, and what to do when
    # something can be done; the long notes wrap with the indent.
    undecryptable-causes)
      expect_selected='!encrypted:example.org'
      expect_active_view='room:!encrypted:example.org'
      room_expect_args=(
        --expect-room '!encrypted:example.org'
      )
      room_activity_expect_args=(
        --expect-room-activity '!encrypted:example.org::"cause":"sent_before_you_joined"'
        --expect-room-activity '!encrypted:example.org::"cause":"historical_unverified"'
      )
      room_view_expect_args=(
        --expect-room-view '!encrypted:example.org::* Bob sent an encrypted message that could not be decrypted (sent before you joined)'
        --expect-room-view '!encrypted:example.org::(the sender withheld its key)'
      )
      expect_switchbar_active='room:!encrypted:example.org'
      ;;
    # Bob's reply decrypts once its key comes, in place; his later line
    # still waits for its key.
    room-keys-decrypted)
      expect_selected='!encrypted:example.org'
      expect_active_view='room:!encrypted:example.org'
      append_events_path="$root_dir/tests/ui-state-updates/room-keys-decrypted.json"
      room_view_expect_args=(
        --expect-room-view '!encrypted:example.org::<Alice> when do we ship?'
        --expect-room-view '!encrypted:example.org::at noon, once the build is green'
        --expect-room-view '!encrypted:example.org::* Bob sent an encrypted message that could not be decrypted'
      )
      expect_switchbar_active='room:!encrypted:example.org'
      ;;
    # Bob's edit of his line came before its key; Alice's forged edit of it
    # never shows (the Wine test checks it is not there).
    room-keys-decrypted-edit)
      expect_selected='!encrypted:example.org'
      expect_active_view='room:!encrypted:example.org'
      append_events_path="$root_dir/tests/ui-state-updates/room-keys-decrypted-edit.json"
      room_view_expect_args=(
        --expect-room-view '!encrypted:example.org::<Bob> at noon, once the build is green and tagged (edited)'
        --expect-room-view '!encrypted:example.org::* Bob sent an encrypted message that could not be decrypted'
      )
      expect_switchbar_active='room:!encrypted:example.org'
      ;;
    # Room key file answers, the failure in red.
    room-keys-imported)
      expect_active_view='status'
      select_views=('status')
      expect_error='wrong passphrase, or the file is damaged'
      status_view_expect_args=(
        --expect-status-view '* Room keys exported to C:\Users\me\Documents\mirx-keys.txt. Keep the file and its passphrase safe.'
        --expect-status-view '* Error during room key import: wrong passphrase, or the file is damaged'
        --expect-status-view '* Imported 3 of 5 room keys from C:\Users\me\Documents\mirx-keys.txt (2 already known)'
      )
      ;;
    file-message)
      expect_selected='!media:example.org'
      expect_active_view='room:!media:example.org'
      room_expect_args=(
        --expect-room '!media:example.org'
      )
      room_activity_expect_args=(
        --expect-room-activity '!media:example.org::"type":"file"'
        --expect-room-activity '!media:example.org::$file1:example.org'
        --expect-room-activity '!media:example.org::report.pdf'
        --expect-room-activity '!media:example.org::mxc://example.org/report'
        --expect-room-activity '!media:example.org::application/pdf'
        --expect-room-activity '!media:example.org::45678'
      )
      room_view_expect_args=(
        --expect-room-view '!media:example.org::* Now talking in Media (!media:example.org)'
        --expect-room-view '!media:example.org::[16:00] * @alice:example.org sent file: report.pdf (application/pdf, 45678 bytes)'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 1 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!media:example.org::Media'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!media:example.org'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!media:example.org'
      view_forbid_args=(
        --expect-no-view 'room:!media:example.org::unsupported'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text 'report.pdf'
      )
      ;;
    audio-message)
      expect_selected='!media:example.org'
      expect_active_view='room:!media:example.org'
      room_expect_args=(
        --expect-room '!media:example.org'
      )
      room_activity_expect_args=(
        --expect-room-activity '!media:example.org::"type":"audio"'
        --expect-room-activity '!media:example.org::$audio1:example.org'
        --expect-room-activity '!media:example.org::voice-note.ogg'
        --expect-room-activity '!media:example.org::mxc://example.org/audio'
        --expect-room-activity '!media:example.org::audio/ogg'
        --expect-room-activity '!media:example.org::23456'
        --expect-room-activity '!media:example.org::12000'
      )
      room_view_expect_args=(
        --expect-room-view '!media:example.org::* Now talking in Media (!media:example.org)'
        --expect-room-view '!media:example.org::[16:01] * @alice:example.org sent audio: voice-note.ogg (audio/ogg, 23456 bytes, 12000 ms)'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 1 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!media:example.org::Media'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!media:example.org'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!media:example.org'
      view_forbid_args=(
        --expect-no-view 'room:!media:example.org::unsupported'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text 'voice-note.ogg'
      )
      ;;
    video-message|video-message-loaded|video-message-failed)
      expect_selected='!media:example.org'
      expect_active_view='room:!media:example.org'
      room_expect_args=(
        --expect-room '!media:example.org'
      )
      room_activity_expect_args=(
        --expect-room-activity '!media:example.org::"type":"video"'
        --expect-room-activity '!media:example.org::$video1:example.org'
        --expect-room-activity '!media:example.org::clip.mp4'
        --expect-room-activity '!media:example.org::mxc://example.org/video'
        --expect-room-activity '!media:example.org::video/mp4'
        --expect-room-activity '!media:example.org::34567'
        --expect-room-activity '!media:example.org::12000'
        --expect-room-activity '!media:example.org::1920'
        --expect-room-activity '!media:example.org::1080'
      )
      if [[ "$case_name" == video-message ]]; then
        room_activity_expect_args+=(
          --expect-room-activity '!media:example.org::"media_state":{"type":"pending"}'
        )
      elif [[ "$case_name" == video-message-loaded ]]; then
        room_activity_expect_args+=(
          --expect-room-activity '!media:example.org::"media_state":{"type":"loaded","width":64,"height":48}'
        )
      else
        room_activity_expect_args+=(
          --expect-room-activity '!media:example.org::"media_state":{"type":"failed","message":"failed to fetch room image media: unavailable"}'
        )
      fi
      room_view_expect_args=(
        --expect-room-view '!media:example.org::* Now talking in Media (!media:example.org)'
        --expect-room-view '!media:example.org::[16:02] * @alice:example.org sent video: clip.mp4 (video/mp4, 1920x1080, 34567 bytes, 12000 ms)'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 1 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!media:example.org::Media'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!media:example.org'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!media:example.org'
      view_forbid_args=(
        --expect-no-view 'room:!media:example.org::unsupported'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text 'clip.mp4'
      )
      ;;
    image-message)
      expect_selected='!images:example.org'
      expect_active_view='room:!images:example.org'
      room_expect_args=(
        --expect-room '!images:example.org'
      )
      room_activity_expect_args=(
        --expect-room-activity '!images:example.org::"type":"image"'
        --expect-room-activity '!images:example.org::diagram.png'
        --expect-room-activity '!images:example.org::"media_state":{"type":"pending"}'
        --expect-room-activity '!images:example.org::mxc://example.org/diagram'
        --expect-room-activity '!images:example.org::image/png'
        --expect-room-activity '!images:example.org::640'
        --expect-room-activity '!images:example.org::480'
        --expect-room-activity '!images:example.org::12345'
      )
      room_view_expect_args=(
        --expect-room-view '!images:example.org::* Now talking in Images (!images:example.org)'
        --expect-room-view '!images:example.org::[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 1 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!images:example.org::Images'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!images:example.org'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!images:example.org'
      navigation_forbid_args=(
        --expect-no-navigation-text 'diagram.png'
      )
      ;;
    image-message-loaded)
      expect_selected='!images:example.org'
      expect_active_view='room:!images:example.org'
      room_expect_args=(
        --expect-room '!images:example.org'
      )
      room_activity_expect_args=(
        --expect-room-activity '!images:example.org::"type":"image"'
        --expect-room-activity '!images:example.org::diagram.png'
        --expect-room-activity '!images:example.org::"media_state":{"type":"loaded","width":64,"height":48}'
        --expect-room-activity '!images:example.org::mxc://example.org/diagram'
        --expect-room-activity '!images:example.org::image/png'
      )
      room_view_expect_args=(
        --expect-room-view '!images:example.org::* Now talking in Images (!images:example.org)'
        --expect-room-view '!images:example.org::[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 1 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!images:example.org::Images'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!images:example.org'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!images:example.org'
      navigation_forbid_args=(
        --expect-no-navigation-text 'diagram.png'
      )
      ;;
    stickers-and-polls)
      expect_selected='!fun:example.org'
      expect_active_view='room:!fun:example.org'
      room_expect_args=(
        --expect-room '!fun:example.org'
      )
      room_activity_expect_args=(
        --expect-room-activity '!fun:example.org::"type":"sticker"'
        --expect-room-activity '!fun:example.org::"media_state":{"type":"loaded","width":64,"height":48}'
        --expect-room-activity '!fun:example.org::"type":"poll"'
        --expect-room-activity '!fun:example.org::"type":"verification_request"'
      )
      room_view_expect_args=(
        --expect-room-view '!fun:example.org::[16:00] * Bob sent sticker: a waving cat'
        --expect-room-view '!fun:example.org::[16:01] * Bob started a poll: Lunch? 1) Pizza 2) Sushi 3) Salad'
        --expect-room-view '!fun:example.org::[16:02] * Bob ended the poll "Lunch?": The poll has ended. Top answer: Pizza'
        --expect-room-view '!fun:example.org::[16:03] * Bob sent unsupported m.location: Big Ben, London'
        --expect-room-view '!fun:example.org::[16:04] * Bob wants to verify @alice:example.org'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 1 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!fun:example.org::Fun'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!fun:example.org'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!fun:example.org'
      ;;
    image-message-failed)
      expect_selected='!images:example.org'
      expect_active_view='room:!images:example.org'
      room_expect_args=(
        --expect-room '!images:example.org'
      )
      room_activity_expect_args=(
        --expect-room-activity '!images:example.org::"type":"image"'
        --expect-room-activity '!images:example.org::diagram.png'
        --expect-room-activity '!images:example.org::"media_state":{"type":"failed","message":"failed to fetch room image media: unavailable"}'
        --expect-room-activity '!images:example.org::mxc://example.org/diagram'
        --expect-room-activity '!images:example.org::image/png'
      )
      room_view_expect_args=(
        --expect-room-view '!images:example.org::* Now talking in Images (!images:example.org)'
        --expect-room-view '!images:example.org::[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 1 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!images:example.org::Images'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!images:example.org'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!images:example.org'
      navigation_forbid_args=(
        --expect-no-navigation-text 'diagram.png'
      )
      ;;
    encrypted-image-message)
      expect_selected='!images:example.org'
      expect_active_view='room:!images:example.org'
      room_expect_args=(
        --expect-room '!images:example.org'
      )
      room_activity_expect_args=(
        --expect-room-activity '!images:example.org::"type":"image"'
        --expect-room-activity '!images:example.org::diagram.png'
        --expect-room-activity '!images:example.org::"type":"encrypted"'
        --expect-room-activity '!images:example.org::"media_state":{"type":"pending"}'
        --expect-room-activity '!images:example.org::mxc://example.org/encrypted-diagram-full'
        --expect-room-activity '!images:example.org::mxc://example.org/encrypted-diagram-thumb'
        --expect-room-activity '!images:example.org::image/png'
        --expect-room-activity '!images:example.org::640'
        --expect-room-activity '!images:example.org::480'
        --expect-room-activity '!images:example.org::12345'
      )
      room_view_expect_args=(
        --expect-room-view '!images:example.org::* Now talking in Images (!images:example.org)'
        --expect-room-view '!images:example.org::[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 1 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!images:example.org::Images'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!images:example.org'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!images:example.org'
      navigation_forbid_args=(
        --expect-no-navigation-text 'diagram.png'
      )
      ;;
    encrypted-image-message-loaded)
      expect_selected='!images:example.org'
      expect_active_view='room:!images:example.org'
      room_expect_args=(
        --expect-room '!images:example.org'
      )
      room_activity_expect_args=(
        --expect-room-activity '!images:example.org::"type":"image"'
        --expect-room-activity '!images:example.org::diagram.png'
        --expect-room-activity '!images:example.org::"type":"encrypted"'
        --expect-room-activity '!images:example.org::"media_state":{"type":"loaded","width":64,"height":48}'
        --expect-room-activity '!images:example.org::mxc://example.org/encrypted-diagram-full'
        --expect-room-activity '!images:example.org::mxc://example.org/encrypted-diagram-thumb'
        --expect-room-activity '!images:example.org::image/png'
        --expect-room-activity '!images:example.org::640'
        --expect-room-activity '!images:example.org::480'
      )
      room_view_expect_args=(
        --expect-room-view '!images:example.org::* Now talking in Images (!images:example.org)'
        --expect-room-view '!images:example.org::[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 1 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!images:example.org::Images'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!images:example.org'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!images:example.org'
      navigation_forbid_args=(
        --expect-no-navigation-text 'diagram.png'
      )
      ;;
    encrypted-image-message-failed)
      expect_selected='!images:example.org'
      expect_active_view='room:!images:example.org'
      room_expect_args=(
        --expect-room '!images:example.org'
      )
      room_activity_expect_args=(
        --expect-room-activity '!images:example.org::"type":"image"'
        --expect-room-activity '!images:example.org::diagram.png'
        --expect-room-activity '!images:example.org::"type":"encrypted"'
        --expect-room-activity '!images:example.org::"media_state":{"type":"failed","message":"failed to fetch room image media: unavailable"}'
        --expect-room-activity '!images:example.org::mxc://example.org/encrypted-diagram-full'
        --expect-room-activity '!images:example.org::mxc://example.org/encrypted-diagram-thumb'
        --expect-room-activity '!images:example.org::image/png'
        --expect-room-activity '!images:example.org::640'
        --expect-room-activity '!images:example.org::480'
      )
      room_view_expect_args=(
        --expect-room-view '!images:example.org::* Now talking in Images (!images:example.org)'
        --expect-room-view '!images:example.org::[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)'
      )
      status_bar_expect_args=(
        --expect-status-bar 'synced 1 room(s)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!images:example.org::Images'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!images:example.org'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!images:example.org'
      navigation_forbid_args=(
        --expect-no-navigation-text 'diagram.png'
      )
      ;;
    *)
      die "no expectations configured for snapshot case: $case_name"
      ;;
  esac
}

run_case() {
  local case_name=$1
  local base_case=$case_name
  local prefix=$WINEPREFIX
  local dpi_expect_args=()
  if is_dpi144_case "$case_name"; then
    base_case=${case_name%-dpi144}
    prefix=$dpi144_prefix
    dpi_expect_args=(--expect-dpi 144)
  fi
  local state_path="$states_dir/$base_case.json"
  local state_path_win
  local baseline_png="$baseline_dir/$case_name.png"
  local generated_png="$generated_dir/$case_name.png"
  local diff_png="$diff_dir/$case_name.diff.png"
  local driver_stdout="$tmpdir/$case_name.driver.stdout"
  local driver_stderr="$tmpdir/$case_name.driver.stderr"
  local driver_status=0
  local driver_args=()
  local ui_args=()
  local append_events_path_win=
  local action=
  local action_path_win=

  configure_case_expectations "$base_case"
  if [[ $case_name != "$base_case" ]] && [[ -n "${case_frame_size:-}" ]]; then
    die "$case_name: a 144-dpi case lays out at the default frame size, but $base_case sets one"
  fi
  cleanup_app
  state_path_win=$(unix_to_wine_path "$state_path")
  driver_args=(
    --find-timeout-ms 30000
    --wait-idle-ms 5000
    --expect-active-view "$expect_active_view"
    "${room_expect_args[@]}"
    "${room_membership_expect_args[@]}"
    "${room_forbid_args[@]}"
    "${room_direct_expect_args[@]}"
    "${room_tag_expect_args[@]}"
    "${room_read_target_expect_args[@]}"
    "${room_unread_expect_args[@]}"
    "${room_activity_expect_args[@]}"
    "${view_expect_args[@]}"
    "${view_forbid_args[@]}"
    "${room_view_expect_args[@]}"
    "${room_member_expect_args[@]}"
    "${status_view_expect_args[@]}"
    "${status_view_forbid_args[@]}"
    "${status_bar_expect_args[@]}"
    "${status_bar_forbid_args[@]}"
    "${switchbar_item_expect_args[@]}"
    "${switchbar_no_item_expect_args[@]}"
    "${switchbar_activity_expect_args[@]}"
    "${switchbar_no_activity_expect_args[@]}"
    "${navigation_expect_args[@]}"
    "${navigation_forbid_args[@]}"
    "${navigation_activity_expect_args[@]}"
    "${navigation_no_activity_expect_args[@]}"
    "${navigation_node_expect_args[@]}"
    "${mdi_placement_expect_args[@]}"
    "${mdi_title_expect_args[@]}"
  )

  if [[ -n "${expect_selected:-}" ]]; then
    driver_args+=(--expect-selected "$expect_selected")
  fi
  if [[ ${#select_views[@]} -gt 0 ]]; then
    local select_view=
    for select_view in "${select_views[@]}"; do
      driver_args+=(--select-view "$select_view")
    done
  fi
  if [[ -n "${trigger_history_backfill:-}" ]]; then
    driver_args+=(--trigger-history-backfill)
  fi
  if [[ -n "${append_events_path:-}" ]]; then
    append_events_path_win=$(unix_to_wine_path "$append_events_path")
    driver_args+=(--append-events "$append_events_path_win")
  fi
  if [[ -n "${find_active_view_query:-}" ]]; then
    driver_args+=(--find-active-view "$find_active_view_query")
  fi
  for action in "${ordered_actions[@]}"; do
    if [[ "$action" == append:* ]]; then
      action_path_win=$(unix_to_wine_path "${action#append:}")
      driver_args+=(--action "append:$action_path_win")
    else
      driver_args+=(--action "$action")
    fi
  done
  if [[ -n "${expect_active_find_history:-}" ]]; then
    driver_args+=(--expect-active-find-history "$expect_active_find_history")
  fi
  if [[ -n "${expect_active_find_selected_text:-}" ]]; then
    driver_args+=(--expect-active-find-selected-text "$expect_active_find_selected_text")
  fi
  if [[ -n "${expect_error:-}" ]]; then
    driver_args+=(--expect-error "$expect_error")
  fi
  if [[ -n "${expect_verification:-}" ]]; then
    driver_args+=(--expect-verification "$expect_verification")
  fi
  if [[ -n "${expect_verification_state:-}" ]]; then
    driver_args+=(--expect-verification-state "$expect_verification_state")
  fi
  driver_args+=("${verification_text_expect_args[@]}")
  if [[ -n "${expect_switchbar_active:-}" ]]; then
    driver_args+=(--expect-switchbar-active "$expect_switchbar_active")
  fi
  if [[ -n "${expect_switchbar_rows:-}" ]]; then
    driver_args+=(--expect-switchbar-rows "$expect_switchbar_rows")
  fi
  if [[ -n "${expect_dialog_title:-}" ]]; then
    driver_args+=(--expect-dialog-title "$expect_dialog_title")
  fi
  driver_args+=("${frame_bar_hidden_expect_args[@]}")
  driver_args+=("${toolbar_button_expect_args[@]}")
  driver_args+=("${dpi_expect_args[@]}")

  ui_args=(--render-state "$state_path_win" --automation)
  if [[ -n "${case_frame_size:-}" ]]; then
    ui_args+=(--frame-size "$case_frame_size")
  fi

  # The driver talks to the app through the prefix's wineserver.
  WINEPREFIX=$prefix wine "$ui_exe" "${ui_args[@]}" >/dev/null 2>&1 &
  app_pid=$!

  set +e
  WINEPREFIX=$prefix wine "$driver_exe" "${driver_args[@]}" >"$driver_stdout" 2>"$driver_stderr"
  driver_status=$?
  set -e

  if [[ -s "$driver_stdout" ]]; then
    cat "$driver_stdout"
  fi

  if [[ $driver_status -ne 0 ]]; then
    if [[ -s "$driver_stderr" ]]; then
      cat "$driver_stderr" >&2
    fi
    cleanup_app
    return "$driver_status"
  fi

  if ! import -window root "$generated_png"; then
    cleanup_app
    return 1
  fi
  cleanup_app
  crop_to_frame "$case_name" "$generated_png" "$(snapshot_frame_rect "$case_name")" || return 1
  echo "PNG $generated_png"

  if [[ ! -f "$baseline_png" ]]; then
    echo "missing UI snapshot baseline: $baseline_png" >&2
    echo "generated: $generated_png" >&2
    accept_or_reject_snapshot "$baseline_png" "$generated_png"
    return
  fi

  if ! compare_snapshot_pngs "$baseline_png" "$generated_png" "$diff_png"; then
    accept_or_reject_snapshot "$baseline_png" "$generated_png"
    return
  fi
}

# The frame's outer size, which the snapshot keeps from 0,0: the case's
# frame size, else the default frame (1024x768; at 144 dpi the scaled frame
# is clamped to the 1280x1024 screen).
snapshot_frame_rect() {
  local case_name=$1
  if [[ -n "${case_frame_size:-}" ]]; then
    echo "$case_frame_size"
  elif is_dpi144_case "$case_name"; then
    echo 1280x1024
  else
    echo 1024x768
  fi
}

# Crops the root capture to the frame's rect, failing if anything was drawn
# outside it (a menu, tooltip or dialog spilling out of the frame): such a
# case needs a larger rect, never a silent cut.
crop_to_frame() {
  local case_name=$1
  local png=$2
  local rect=$3
  local width=${rect%x*}
  local height=${rect#*x}
  local root_width root_height outside
  read -r root_width root_height < <(magick identify -format '%w %h\n' "$png")
  ((width <= root_width && height <= root_height)) \
    || die "$case_name: frame rect $rect is larger than the screen ${root_width}x${root_height}"
  if ((width < root_width)); then
    outside=$(magick "$png" -alpha off -crop "$((root_width - width))x${root_height}+${width}+0" +repage -format '%[max]' info:)
    if [[ $outside != 0 ]]; then
      echo "$case_name: something is drawn right of the frame rect $rect: $png" >&2
      return 1
    fi
  fi
  if ((height < root_height)); then
    outside=$(magick "$png" -alpha off -crop "${width}x$((root_height - height))+0+${height}" +repage -format '%[max]' info:)
    if [[ $outside != 0 ]]; then
      echo "$case_name: something is drawn below the frame rect $rect: $png" >&2
      return 1
    fi
  fi
  magick "$png" -crop "${width}x${height}+0+0" +repage "$png"
}

# Status 0 = the generated PNG became the baseline (update mode), 1 = failure.
accept_or_reject_snapshot() {
  local baseline_png="$1"
  local generated_png="$2"

  if [[ "${MIRX_SNAPSHOT_UPDATE:-0}" != "1" ]]; then
    keep_tmpdir=1
    return 1
  fi
  if [[ -f "$baseline_png" ]]; then
    echo "diff bbox: $(magick "$baseline_png" "$generated_png" -compose difference -composite -threshold 0 -format '%@' info:)" >&2
  fi
  cp "$generated_png" "$baseline_png" || return 1
  updated_cases+=("$(basename "$baseline_png" .png)")
  echo "updated baseline: $baseline_png" >&2
}

compare_snapshot_pngs() {
  local baseline_png="$1"
  local generated_png="$2"
  local diff_png="$3"
  local compare_output ae_metric

  set +e
  compare_output="$(compare -metric AE "$baseline_png" "$generated_png" "$diff_png" 2>&1)"
  set -e

  ae_metric="${compare_output%%[[:space:]]*}"

  if [[ "$ae_metric" != "0" ]]; then
    echo "visual snapshot mismatch" >&2
    echo "baseline:  $baseline_png" >&2
    echo "generated: $generated_png" >&2
    echo "diff:      $diff_png" >&2
    echo "metric:    $compare_output" >&2
    return 1
  fi

  return 0
}

main() {
  require_command wine
  require_command wineboot
  require_command Xvfb
  require_command import
  require_command compare
  require_command magick
  require_command xdpyinfo

  [[ -d "$states_dir" ]] || die "missing states directory: $states_dir"

  tmpdir=$(mktemp -d "${TMPDIR:-/var/tmp}/mirx-ui-snapshot.XXXXXX")
  generated_dir="$tmpdir/generated-ui-snapshots"
  diff_dir="$tmpdir/ui-snapshot-diffs"
  keep_tmpdir=0
  mkdir -p "$generated_dir" "$diff_dir"
  trap cleanup EXIT

  cd "$root_dir"
  ensure_xvfb
  resolve_binaries
  ensure_wineprefix "$tmpdir/wineprefix"
  configure_snapshot_prefix
  ensure_binaries

  shopt -s nullglob
  local state_files=("$states_dir"/*.json)
  shopt -u nullglob
  [[ ${#state_files[@]} -gt 0 ]] || die "no state files found in $states_dir"

  local cases=()
  local case_name=
  if [[ $# -gt 0 ]]; then
    for case_name in "$@"; do
      is_dpi144_case "$case_name" || [[ -f "$states_dir/$case_name.json" ]] \
        || die "unknown snapshot case: $case_name"
      cases+=("$case_name")
    done
  else
    local state_path=
    for state_path in "${state_files[@]}"; do
      case_name=${state_path##*/}
      case_name=${case_name%.json}
      if is_default_snapshot_case "$case_name"; then
        cases+=("$case_name")
      fi
    done
    for case_name in "${dpi144_cases[@]}"; do
      cases+=("$case_name-dpi144")
    done
  fi
  for case_name in "${cases[@]}"; do
    if is_dpi144_case "$case_name"; then
      configure_dpi144_prefix
      break
    fi
  done

  # Every case runs; failures are collected and reported together.
  local failed_cases=()
  updated_cases=()
  for case_name in "${cases[@]}"; do
    if ! run_case "$case_name"; then
      failed_cases+=("$case_name")
    fi
  done

  if [[ ${#updated_cases[@]} -gt 0 ]]; then
    echo "UPDATED: ${updated_cases[*]}"
  fi
  if [[ ${#failed_cases[@]} -gt 0 ]]; then
    echo "FAILED: ${failed_cases[*]}" >&2
    return 1
  fi
  if [[ ${#updated_cases[@]} -eq 0 ]]; then
    echo "all ${#cases[@]} UI snapshot cases passed"
  fi
}

main "$@"
