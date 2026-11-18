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

die() {
  echo "ui-snapshot: $*" >&2
  exit 1
}

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

ensure_wineprefix() {
  export WINEPREFIX="${WINEPREFIX:-$tmpdir/wineprefix}"
  export WINEARCH="${WINEARCH:-win64}"
  export WINEDLLOVERRIDES=mscoree,mshtml=
  export WINEDEBUG="${WINEDEBUG:--all}"

  mkdir -p "$WINEPREFIX"
  if [[ ! -f "$WINEPREFIX/system.reg" ]]; then
    wineboot -i >/dev/null 2>&1 || die "wineboot failed while initializing $WINEPREFIX"
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
  verification_text_expect_args=()
  mdi_placement_expect_args=()
  mdi_title_expect_args=()

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
        --expect-mdi-title 'room:!ops:example::Ops [e2ee]: incident review'
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
        --expect-mdi-title 'room:!ops:example::Ops [e2ee]: incident review'
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
        --expect-room-view '!invite:example::* You are invited to join Lobby'
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
        --expect-navigation-text 'rooms: 1'
        --expect-navigation-text 'invites: 1'
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
        --expect-room-view '!invite-direct:example::* You are invited to join Daria'
        --expect-room-view '!invite-direct:example::Use /accept or /reject to respond.'
      )
      status_bar_expect_args=(
        --expect-status-bar 'invited: Daria (!invite-direct:example)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!beta:example::Beta'
        --expect-switchbar-item 'room:!invite-direct:example::Daria [invite] [query: @daria:example]'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!beta:example'
        --expect-no-switchbar-activity 'room:!invite-direct:example'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!invite-direct:example'
      navigation_expect_args=(
        --expect-navigation-text 'rooms: 1'
        --expect-navigation-text 'invites: 1'
        --expect-navigation-text 'Beta'
        --expect-navigation-text 'Daria [invite] [query: @daria:example]'
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
        --expect-navigation-text 'rooms: 2'
        --expect-navigation-text 'invites: 0'
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
        --expect-navigation-text 'rooms: 1'
        --expect-navigation-text 'invites: 0'
        --expect-navigation-text 'Beta'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text 'Lobby [invite]'
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
        --expect-switchbar-item 'room:!direct:example::Direct [query: @alice:example] [e2ee]'
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
        --expect-navigation-text 'unspaced rooms: 1'
        --expect-navigation-text 'Ops [e2ee]'
        --expect-navigation-text 'Shared'
        --expect-navigation-text 'Random'
        --expect-navigation-text 'Direct [query: @alice:example] [e2ee]'
      )
      navigation_forbid_args=(
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
        --expect-status-bar 'find: 1/2 "thread needle"'
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
        --expect-status-view '[22:12] * E2EE: recovery enabled, local backup enabled, server backup not checked, cross-signing complete'
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
        --expect-switchbar-item 'status::Status'
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
        --expect-room-activity '!reactions:example::"count":1'
        --expect-room-activity '!reactions:example::$reaction-plus-one-b'
        --expect-room-activity '!reactions:example::"key":"eyes"'
        --expect-room-activity '!reactions:example::"type":"thread"'
      )
      room_view_expect_args=(
        --expect-room-view '!reactions:example::* Now talking in Reactions (!reactions:example)'
        --expect-room-view '!reactions:example::normal reaction target'
        --expect-room-view '!reactions:example::reactions: +1 x1'
      )
      view_expect_args=(
        --expect-view 'thread:!reactions:example|$react-thread-root::thread reaction root'
        --expect-view 'thread:!reactions:example|$react-thread-root::thread reaction reply'
        --expect-view 'thread:!reactions:example|$react-thread-root::reactions: eyes x1'
      )
      view_forbid_args=(
        --expect-no-view 'room:!reactions:example::heart x1'
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
        --expect-status-view '* Actions: Account > Accept Verification, Account > Cancel Verification, /verify accept, /verify cancel'
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
        --expect-status-view '* Actions: Account > Accept Verification, Account > Cancel Verification, /verify accept, /verify cancel'
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
        --expect-status-view '* Actions: Account > Confirm SAS, Account > Reject SAS, Account > Cancel Verification, /verify confirm, /verify reject, /verify cancel'
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
        --expect-status-view '[22:12] * E2EE: recovery incomplete, local backup disabled, server backup exists, cross-signing incomplete'
        --expect-status-view '* Use /e2ee refresh to check server backup, /e2ee setup to enable recovery, /e2ee recover to restore keys'
      )
      status_view_forbid_args=(
        --expect-no-status-view 'recovery_secret'
        --expect-no-status-view 'passphrase='
      )
      status_bar_expect_args=(
        --expect-status-bar 'e2ee: recovery incomplete, local backup disabled, cross-signing incomplete'
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
        --expect-status-view '[22:12] * E2EE: recovery enabled, local backup enabled, server backup exists, cross-signing complete'
        --expect-status-view '* Use /e2ee refresh to check server backup, /e2ee setup to enable recovery, /e2ee recover to restore keys'
      )
      status_view_forbid_args=(
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
        --expect-status-view '[22:12] * E2EE: recovery incomplete, local backup disabled, server backup exists, cross-signing incomplete'
        --expect-status-view '* Use /e2ee refresh to check server backup, /e2ee setup to enable recovery, /e2ee recover to restore keys'
        --expect-status-view '[22:12] * Error during e2ee recovery: failed to recover E2EE secrets: invalid recovery key or passphrase'
      )
      status_view_forbid_args=(
        --expect-no-status-view 'recovery_secret'
        --expect-no-status-view 'passphrase='
      )
      status_bar_expect_args=(
        --expect-status-bar 'e2ee: recovery incomplete, local backup disabled, cross-signing incomplete'
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
        --expect-status-view '[22:12] * E2EE: recovery enabled, local backup enabled, server backup missing, cross-signing complete'
        --expect-status-view '* Use /e2ee refresh to check server backup, /e2ee setup to enable recovery, /e2ee recover to restore keys'
      )
      status_view_forbid_args=(
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
        --expect-room-view '!mixed:example::[22:16] * Parts: @carol:example'
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
        --expect-room-view '!dense:example::[22:17] * Parts: @dense-leaver:example'
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
        --expect-no-navigation-activity 'History'
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
        --expect-no-navigation-activity 'History'
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
        --expect-no-navigation-activity 'History'
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
        --expect-no-navigation-activity 'History'
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
        --expect-navigation-activity 'Beta::message'
        --expect-navigation-activity 'Alpha/alpha root::message'
      )
      navigation_no_activity_expect_args=(
        --expect-no-navigation-activity 'status'
        --expect-no-navigation-activity 'Alpha'
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
        --expect-navigation-activity 'Events::event'
        --expect-navigation-activity 'Chat::message'
        --expect-navigation-activity 'Pings::highlight'
        --expect-navigation-activity 'Casey::message'
        --expect-navigation-activity 'Counts::highlight'
      )
      navigation_no_activity_expect_args=(
        --expect-no-navigation-activity 'status'
        --expect-no-navigation-activity 'Lobby'
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
        --expect-switchbar-item 'room:!query:example::Casey [query: @casey:example] [e2ee]'
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
        --expect-navigation-text 'Casey [query: @casey:example] [e2ee]'
        --expect-navigation-text 'review thread root'
      )
      navigation_activity_expect_args=(
        --expect-navigation-activity 'Alerts::message'
        --expect-navigation-activity 'Support Queue::message'
        --expect-navigation-activity 'Focus/review thread root::message'
      )
      expect_switchbar_active='room:!focus:example'
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
        --expect-switchbar-item 'room:!long-query:example::@very-long-direct-user-identifier-for-constrained-shell:example [query: @very-long-direct-user-identifier-for-constrained-shell:example] [e2ee]'
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
        --expect-navigation-text '@very-long-direct-user-identifier-for-constrained-shell:example [query: @very-long-direct-user-identifier-for-constrained-shell:example] [e2ee]'
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
        --expect-navigation-activity 'Inbox::message'
        --expect-navigation-activity 'Mentions::message'
        --expect-navigation-activity 'Active Focus/active thread root::message'
      )
      navigation_no_activity_expect_args=(
        --expect-no-navigation-activity 'status'
        --expect-no-navigation-activity 'Active Focus'
      )
      expect_switchbar_active='room:!active:example'
      ;;
    membership-changes)
      expect_selected='!membership:example'
      expect_active_view='room:!membership:example'
      room_expect_args=(
        --expect-room '!membership:example'
      )
      room_activity_expect_args=(
        --expect-room-activity '!membership:example::kicked_and_banned'
        --expect-room-activity '!membership:example::display_name_changed'
        --expect-room-activity '!membership:example::flooding'
      )
      room_view_expect_args=(
        --expect-room-view '!membership:example::* Now talking in Membership (!membership:example)'
        --expect-room-view '!membership:example::[22:13] * Joins: Carol (@carol:example)'
        --expect-room-view '!membership:example::[22:14] * Parts: @dave:example'
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
        --expect-switchbar-item 'room:!alpha:example::Alpha [query: @alice:example] [@1 !2 *1 M]'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!alpha:example::highlight'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!beta:example'
        --expect-no-switchbar-activity 'status'
      )
      navigation_expect_args=(
        --expect-navigation-text 'Alpha [query: @alice:example] [@1 !2 *1 M]'
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
        --expect-switchbar-item 'room:!alpha:example::Alpha [query: @alice:example]'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!alpha:example'
        --expect-no-switchbar-activity 'room:!beta:example'
        --expect-no-switchbar-activity 'status'
      )
      navigation_expect_args=(
        --expect-navigation-text 'Alpha [query: @alice:example]'
      )
      navigation_forbid_args=(
        --expect-no-navigation-text 'Alpha [query: @alice:example] [@1 !2 *1 M]'
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
      expect_selected='!alpha:example.org'
      expect_active_view='room:!alpha:example.org'
      room_expect_args=(
        --expect-room '!alpha:example.org'
        --expect-room '!encrypted:example.org'
      )
      room_activity_expect_args=(
        --expect-room-activity '!encrypted:example.org::unable_to_decrypt'
        --expect-room-activity '!encrypted:example.org::$encrypted-utd1:example.org'
        --expect-room-activity '!encrypted:example.org::"reason":"missing_megolm_session"'
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
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!encrypted:example.org::message'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!alpha:example.org'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!alpha:example.org'
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
  local state_path=$1
  local state_path_win
  local case_name=${state_path##*/}
  case_name=${case_name%.json}
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

  configure_case_expectations "$case_name"
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

  ui_args=(--render-state "$state_path_win" --automation)
  if [[ -n "${case_frame_size:-}" ]]; then
    ui_args+=(--frame-size "$case_frame_size")
  fi

  wine "$ui_exe" "${ui_args[@]}" >/dev/null 2>&1 &
  app_pid=$!

  set +e
  wine "$driver_exe" "${driver_args[@]}" >"$driver_stdout" 2>"$driver_stderr"
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
  ensure_wineprefix
  ensure_binaries

  shopt -s nullglob
  local state_files=("$states_dir"/*.json)
  shopt -u nullglob
  [[ ${#state_files[@]} -gt 0 ]] || die "no state files found in $states_dir"

  if [[ $# -gt 0 ]]; then
    local requested=()
    local case_name=
    for case_name in "$@"; do
      local state_path="$states_dir/$case_name.json"
      [[ -f "$state_path" ]] || die "unknown snapshot case: $case_name"
      requested+=("$state_path")
    done
    state_files=("${requested[@]}")
  else
    local discovered=()
    local state_path=
    for state_path in "${state_files[@]}"; do
      local case_name=${state_path##*/}
      case_name=${case_name%.json}
      if is_default_snapshot_case "$case_name"; then
        discovered+=("$state_path")
      fi
    done
    state_files=("${discovered[@]}")
  fi

  # Every case runs; failures are collected and reported together.
  local failed_cases=()
  updated_cases=()
  for state_path in "${state_files[@]}"; do
    local case_name=${state_path##*/}
    if ! run_case "$state_path"; then
      failed_cases+=("${case_name%.json}")
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
    echo "all ${#state_files[@]} UI snapshot cases passed"
  fi
}

main "$@"
