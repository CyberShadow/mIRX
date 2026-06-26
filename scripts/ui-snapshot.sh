#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
target_dir="$root_dir/target/x86_64-pc-windows-gnu/debug"
ui_exe="$target_dir/mmatrix-ui.exe"
driver_exe="$target_dir/mmatrix-snapshot-driver.exe"
states_dir="$root_dir/tests/ui-states"
artifacts_dir="$root_dir/artifacts/ui-snapshots"

started_xvfb=0
xvfb_pid=
tmpdir=
app_pid=

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
    export DISPLAY=$display
    return 0
  fi

  cleanup_stale_display_files "$display"
  export DISPLAY=$display
  Xvfb "$DISPLAY" -screen 0 1280x1024x24 >"$tmpdir/xvfb.log" 2>&1 &
  xvfb_pid=$!
  started_xvfb=1
  wait_for_display "$DISPLAY" || die "Xvfb did not become healthy on $DISPLAY"
}

ensure_xvfb() {
  if [[ -n "${DISPLAY:-}" ]] && display_is_healthy "$DISPLAY"; then
    return 0
  fi

  if [[ -n "${MMATRIX_WINE_DISPLAY:-}" ]]; then
    start_xvfb_explicit "$MMATRIX_WINE_DISPLAY"
  else
    start_xvfb_auto
  fi
}

ensure_wineprefix() {
  export WINEPREFIX="${WINEPREFIX:-$root_dir/.wine-mmatrix-snapshots}"
  export WINEARCH=win64
  export WINEDLLOVERRIDES=mscoree,mshtml=
  export WINEDEBUG=-all

  mkdir -p "$WINEPREFIX"
  if [[ ! -f "$WINEPREFIX/system.reg" ]]; then
    wineboot -i >/dev/null 2>&1 || die "wineboot failed while initializing $WINEPREFIX"
  fi
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
  if [[ -n "${tmpdir:-}" ]]; then
    rm -rf "$tmpdir"
  fi
}

ensure_binaries() {
  if [[ -x "$ui_exe" && -x "$driver_exe" ]]; then
    return 0
  fi

  cargo build --target x86_64-pc-windows-gnu -p mmatrix-ui
  cargo build --target x86_64-pc-windows-gnu -p mmatrix-testkit --bin mmatrix-snapshot-driver
}

configure_case_expectations() {
  local case_name=$1
  timeline_expect_args=()
  room_expect_args=()
  expect_selected=
  expect_error=
  expect_verification=

  case "$case_name" in
    multi-room)
      expect_selected='!ops:example'
      room_expect_args=(
        --expect-room '!lobby:example'
        --expect-room '!ops:example'
        --expect-room '!random:example'
      )
      timeline_expect_args=(
        --expect-timeline '!ops:example::incident review'
        --expect-timeline '!ops:example::encryption enabled'
        --expect-timeline '!ops:example::deploy window opens'
      )
      ;;
    verification-error)
      expect_selected='!verify:example'
      expect_verification='flow-verify-123'
      expect_error='message send failed'
      room_expect_args=(
        --expect-room '!support:example'
        --expect-room '!verify:example'
      )
      timeline_expect_args=(
        --expect-timeline '!verify:example::verification requested'
        --expect-timeline '!verify:example::sas emoji ready'
        --expect-timeline '!verify:example::waiting for confirmation'
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
  local png_path="$artifacts_dir/$case_name.png"
  local driver_stdout="$tmpdir/$case_name.driver.stdout"
  local driver_stderr="$tmpdir/$case_name.driver.stderr"
  local driver_status=0
  local driver_args=()

  configure_case_expectations "$case_name"
  cleanup_app
  state_path_win=$(unix_to_wine_path "$state_path")
  driver_args=(
    --wait-idle-ms 5000
    --expect-selected "$expect_selected"
    "${room_expect_args[@]}"
    "${timeline_expect_args[@]}"
  )

  if [[ -n "${expect_error:-}" ]]; then
    driver_args+=(--expect-error "$expect_error")
  fi
  if [[ -n "${expect_verification:-}" ]]; then
    driver_args+=(--expect-verification "$expect_verification")
  fi

  wine "$ui_exe" --render-state "$state_path_win" --automation >/dev/null 2>&1 &
  app_pid=$!

  set +e
  wine "$driver_exe" "${driver_args[@]}" >"$driver_stdout" 2>"$driver_stderr"
  driver_status=$?
  set -e

  import -window root "$png_path"
  echo "PNG $png_path"

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

  cleanup_app
}

main() {
  require_command wine
  require_command wineboot
  require_command Xvfb
  require_command import
  require_command xdpyinfo

  [[ -d "$states_dir" ]] || die "missing states directory: $states_dir"

  tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/mmatrix-ui-snapshot.XXXXXX")
  trap cleanup EXIT

  cd "$root_dir"
  ensure_xvfb
  ensure_wineprefix
  ensure_binaries

  mkdir -p "$artifacts_dir"

  shopt -s nullglob
  local state_files=("$states_dir"/*.json)
  shopt -u nullglob
  [[ ${#state_files[@]} -gt 0 ]] || die "no state files found in $states_dir"

  for state_path in "${state_files[@]}"; do
    run_case "$state_path"
  done
}

main "$@"
