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
  if [[ -n "${MMATRIX_WINE_DISPLAY:-}" ]]; then
    start_xvfb_explicit "$MMATRIX_WINE_DISPLAY"
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
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$tmpdir/cargo-target}"
  target_dir="$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/debug"
  ui_exe="$target_dir/mmatrix-ui.exe"
  driver_exe="$target_dir/mmatrix-snapshot-driver.exe"
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

ensure_binaries() {
  cargo clean --target x86_64-pc-windows-gnu -p mmatrix-ui -p mmatrix-testkit
  cargo build --target x86_64-pc-windows-gnu -p mmatrix-ui
  cargo build --target x86_64-pc-windows-gnu -p mmatrix-testkit --bin mmatrix-snapshot-driver
}

configure_case_expectations() {
  local case_name=$1
  room_activity_expect_args=()
  room_expect_args=()
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
  expect_selected=
  expect_error=
  expect_verification=
  expect_verification_state=
  expect_active_view=
  select_view=
  expect_switchbar_active=
  verification_text_expect_args=()

  case "$case_name" in
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
        --expect-room-view '!ops:example::Room: Ops [e2ee] (!ops:example)'
        --expect-room-view '!ops:example::Topic: incident review'
        --expect-room-view '!ops:example::* [22:13] @alice:example set topic: incident review'
        --expect-room-view '!ops:example::* [22:14] carol joined'
        --expect-room-view '!ops:example::[22:15] @alice:example: ops topic: incident review'
      )
      status_bar_expect_args=(
        --expect-status-bar 'timeline update from !random:example'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ops:example::Ops'
        --expect-switchbar-item 'room:!lobby:example::Lobby'
        --expect-switchbar-item 'room:!random:example::Random'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!lobby:example'
        --expect-switchbar-activity 'room:!random:example'
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
        --expect-room-view '!ops:example::Room: Ops [e2ee] (!ops:example)'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!ops:example::Ops'
        --expect-switchbar-item 'room:!shared:example::Shared'
        --expect-switchbar-item 'room:!random:example::Random'
        --expect-switchbar-item 'room:!direct:example::Direct'
      )
      switchbar_no_item_expect_args=(
        --expect-no-switchbar-item 'room:!work:example'
        --expect-no-switchbar-item 'room:!social:example'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!shared:example'
        --expect-switchbar-activity 'room:!random:example'
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
        --expect-navigation-text 'Direct [e2ee]'
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
      select_view='thread:!ops:example|$thread-root'
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
        --expect-room-view '!verify:example::Room: Verify [e2ee] (!verify:example)'
        --expect-room-view '!verify:example::[22:14] @alice:example: verification requested by @alice:example'
        --expect-room-view '!verify:example::[22:15] @alice:example: sas emoji ready'
        --expect-room-view '!verify:example::[22:16] @alice:example: waiting for confirmation'
      )
      status_view_expect_args=(
        --expect-status-view 'flow-verify-123'
        --expect-status-view 'message send failed'
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
        --expect-switchbar-activity 'room:!support:example'
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
        --expect-status-view 'status: verification requested by @alice:example'
        --expect-status-view 'state: requested'
        --expect-status-view 'flow_id: flow-verify-request'
        --expect-status-view 'from_user: @alice:example'
        --expect-status-view 'actions: /verify accept, /verify cancel'
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
        --expect-status-view 'status: verification sas ready with @alice:example'
        --expect-status-view 'state: sas ready'
        --expect-status-view 'flow_id: flow-verify-sas-ready'
        --expect-status-view 'from_user: @alice:example'
        --expect-status-view 'emoji: 🐶 dog | 🐱 cat | 🦁 lion | 🐎 horse | 🦄 unicorn | 🐷 pig | 🐘 elephant'
        --expect-status-view 'decimals: 1234-5678-9012'
        --expect-status-view 'actions: /verify confirm, /verify reject, /verify cancel'
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
        --expect-status-view 'status: verification done with @alice:example'
        --expect-status-view 'state: done'
        --expect-status-view 'flow_id: flow-verify-done'
        --expect-status-view 'from_user: @alice:example'
        --expect-status-view 'result: verified'
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
        --expect-status-view 'status: verification cancelled with @alice:example'
        --expect-status-view 'state: cancelled'
        --expect-status-view 'flow_id: flow-verify-cancelled'
        --expect-status-view 'from_user: @alice:example'
        --expect-status-view 'reason: timed out waiting for confirmation'
        --expect-status-view 'code: m.timeout'
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
        --expect-status-view 'e2ee'
        --expect-status-view 'recovery: incomplete'
        --expect-status-view 'backup: enabled'
        --expect-status-view 'backup_enabled_local: false'
        --expect-status-view 'backup_exists_on_server: true'
        --expect-status-view 'cross_signing: incomplete'
        --expect-status-view 'actions: Account > Recover E2EE..., /e2ee refresh'
      )
      status_view_forbid_args=(
        --expect-no-status-view 'recovery_secret'
        --expect-no-status-view 'passphrase='
      )
      status_bar_expect_args=(
        --expect-status-bar 'e2ee: recovery incomplete, backup enabled'
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
        --expect-status-view 'e2ee'
        --expect-status-view 'recovery: enabled'
        --expect-status-view 'backup: enabled'
        --expect-status-view 'backup_enabled_local: true'
        --expect-status-view 'backup_exists_on_server: true'
        --expect-status-view 'cross_signing: complete'
        --expect-status-view 'actions: Account > Recover E2EE..., /e2ee refresh'
      )
      status_view_forbid_args=(
        --expect-no-status-view 'recovery_secret'
        --expect-no-status-view 'passphrase='
      )
      status_bar_expect_args=(
        --expect-status-bar 'e2ee: recovery enabled, backup enabled'
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
        --expect-status-view 'e2ee'
        --expect-status-view 'recovery: incomplete'
        --expect-status-view 'backup: enabled'
        --expect-status-view 'backup_enabled_local: false'
        --expect-status-view 'backup_exists_on_server: true'
        --expect-status-view 'cross_signing: incomplete'
        --expect-status-view 'actions: Account > Recover E2EE..., /e2ee refresh'
        --expect-status-view 'operation: e2ee recovery'
        --expect-status-view 'failed to recover E2EE secrets: invalid recovery key or passphrase'
      )
      status_view_forbid_args=(
        --expect-no-status-view 'recovery_secret'
        --expect-no-status-view 'passphrase='
      )
      status_bar_expect_args=(
        --expect-status-bar 'e2ee: recovery incomplete, backup enabled'
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
        --expect-room-view '!mixed:example::Room: Mixed [e2ee] (!mixed:example)'
        --expect-room-view '!mixed:example::Topic: mixed room topic'
        --expect-room-view '!mixed:example::[22:13] @alice:example: synced hello'
        --expect-room-view '!mixed:example::* [22:14] @alice:example set topic: mixed room topic'
        --expect-room-view '!mixed:example::* [22:15] bob joined'
        --expect-room-view '!mixed:example::* [22:16] @carol:example left'
        --expect-room-view '!mixed:example::[22:17] -@system:example- server maintenance'
        --expect-room-view '!mixed:example::* [22:18] @alice:example waves hello'
        --expect-room-view '!mixed:example::* [22:17] @alice:example pending local echo (sending)'
        --expect-room-view '!mixed:example::* [22:18] @alice:example failed local echo (failed)'
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
    member-list)
      expect_selected='!members:example'
      expect_active_view='room:!members:example'
      room_expect_args=(
        --expect-room '!members:example'
      )
      room_view_expect_args=(
        --expect-room-view '!members:example::Room: Members (!members:example)'
        --expect-room-view '!members:example::member list context line'
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
    typing-indicator)
      expect_selected='!typing:example'
      expect_active_view='room:!typing:example'
      room_expect_args=(
        --expect-room '!typing:example'
      )
      room_view_expect_args=(
        --expect-room-view '!typing:example::Room: Typing (!typing:example)'
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
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!beta:example::Beta'
        --expect-switchbar-item 'room:!alpha:example::Alpha'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!alpha:example'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!beta:example'
        --expect-no-switchbar-activity 'status'
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
        --expect-room-activity '!media:example.org::m.audio'
        --expect-room-activity '!media:example.org::voice-note.ogg'
      )
      room_view_expect_args=(
        --expect-room-view '!media:example.org::Room: Media (!media:example.org)'
        --expect-room-view '!media:example.org::* [16:00] @alice:example.org sent unsupported m.audio: voice-note.ogg'
      )
      status_bar_expect_args=(
        --expect-status-bar 'timeline update from !media:example.org'
      )
      switchbar_item_expect_args=(
        --expect-switchbar-item 'status::Status'
        --expect-switchbar-item 'room:!alpha:example.org::Alpha'
        --expect-switchbar-item 'room:!media:example.org::* Media'
      )
      switchbar_activity_expect_args=(
        --expect-switchbar-activity 'room:!media:example.org'
      )
      switchbar_no_activity_expect_args=(
        --expect-no-switchbar-activity 'room:!alpha:example.org'
        --expect-no-switchbar-activity 'status'
      )
      expect_switchbar_active='room:!alpha:example.org'
      navigation_forbid_args=(
        --expect-no-navigation-text 'voice-note.ogg'
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
        --expect-room-activity '!images:example.org::mxc://example.org/diagram'
        --expect-room-activity '!images:example.org::image/png'
        --expect-room-activity '!images:example.org::640'
        --expect-room-activity '!images:example.org::480'
        --expect-room-activity '!images:example.org::12345'
      )
      room_view_expect_args=(
        --expect-room-view '!images:example.org::Room: Images (!images:example.org)'
        --expect-room-view '!images:example.org::* [16:00] @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)'
      )
      status_bar_expect_args=(
        --expect-status-bar 'timeline update from !images:example.org'
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

  configure_case_expectations "$case_name"
  cleanup_app
  state_path_win=$(unix_to_wine_path "$state_path")
  driver_args=(
    --find-timeout-ms 30000
    --wait-idle-ms 5000
    --expect-active-view "$expect_active_view"
    "${room_expect_args[@]}"
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
  )

  if [[ -n "${expect_selected:-}" ]]; then
    driver_args+=(--expect-selected "$expect_selected")
  fi
  if [[ -n "${select_view:-}" ]]; then
    driver_args+=(--select-view "$select_view")
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

  wine "$ui_exe" --render-state "$state_path_win" --automation >/dev/null 2>&1 &
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

  import -window root "$generated_png"
  echo "PNG $generated_png"

  if [[ ! -f "$baseline_png" ]]; then
    keep_tmpdir=1
    echo "missing UI snapshot baseline: $baseline_png" >&2
    echo "generated: $generated_png" >&2
    cleanup_app
    return 1
  fi

  compare_snapshot_pngs "$baseline_png" "$generated_png" "$diff_png"

  cleanup_app
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
    keep_tmpdir=1
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
  require_command xdpyinfo

  [[ -d "$states_dir" ]] || die "missing states directory: $states_dir"

  tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/mmatrix-ui-snapshot.XXXXXX")
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
  fi

  for state_path in "${state_files[@]}"; do
    run_case "$state_path"
  done
}

main "$@"
