#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
runner_pid=$$
runner_tmpdir=
started_xvfb=0
xvfb_pid=
helper_pid=

die() {
  echo "wine-runner: $*" >&2
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
  local socket_path
  local lock_path
  socket_path=$(display_socket_path "$display")
  lock_path=$(display_lock_path "$display")

  if [[ -S "$socket_path" ]] || [[ -f "$lock_path" ]]; then
    rm -f "$socket_path" "$lock_path"
  fi
}

start_xvfb_auto() {
  local display_file
  display_file=$(mktemp "$runner_tmpdir/xvfb-display.XXXXXX")
  Xvfb -displayfd 3 -screen 0 1024x768x24 3>"$display_file" >"$runner_tmpdir/xvfb.log" 2>&1 &
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
  Xvfb "$DISPLAY" -screen 0 1024x768x24 >"$runner_tmpdir/xvfb.log" 2>&1 &
  xvfb_pid=$!
  started_xvfb=1

  wait_for_display "$DISPLAY" || die "Xvfb did not become healthy on $DISPLAY"
}

ensure_xvfb() {
  if [[ -n "${DISPLAY:-}" ]] && display_is_healthy "$DISPLAY"; then
    return 0
  fi

  if [[ -n "${MIRX_WINE_DISPLAY:-}" ]]; then
    start_xvfb_explicit "$MIRX_WINE_DISPLAY"
  else
    start_xvfb_auto
  fi
}

ensure_wineprefix() {
  export WINEPREFIX="${WINEPREFIX:-$runner_tmpdir/wineprefix}"
  export WINEARCH="${WINEARCH:-win64}"
  export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:-mscoree,mshtml=}"
  export WINEDEBUG="${WINEDEBUG:--all}"

  if [[ ! -f "$WINEPREFIX/system.reg" ]]; then
    local prefix_lock_name=${WINEPREFIX//\//_}
    local prefix_lock_file="${TMPDIR:-/var/tmp}/mirx-wineprefix-init${prefix_lock_name}.lock"
    exec 8>"$prefix_lock_file"
    flock 8
    mkdir -p "$WINEPREFIX"
    if [[ ! -f "$WINEPREFIX/system.reg" ]]; then
      wineboot -i >/dev/null 2>&1 || die "wineboot failed while initializing $WINEPREFIX"
    fi
    flock -u 8
    exec 8>&-
  fi
}

unix_to_wine_path() {
  local unix_path=$1
  printf 'Z:%s' "${unix_path//\//\\}"
}

host_capture_helper() {
  local request_dir=$1
  local request_file
  local work_file
  local capture_name
  local capture_path
  local result_file
  local current_work_file=
  local current_result_file=

  write_interrupted_result() {
    if [[ -n "$current_result_file" ]]; then
      {
        printf 'error\n'
        printf 'host screenshot capture helper terminated while processing request\n'
        printf 'request file: %s\n' "$current_work_file"
        printf 'DISPLAY: %s\n' "${DISPLAY:-unset}"
      } >"$current_result_file"
    fi
  }
  trap 'write_interrupted_result; exit 143' TERM INT HUP

  while :; do
    shopt -s nullglob
    for request_file in "$request_dir"/*.request; do
      work_file=${request_file%.request}.working
      if ! mv "$request_file" "$work_file" 2>/dev/null; then
        continue
      fi

      current_work_file=$work_file
      capture_name=$(tr -d '\r\n' <"$work_file")
      result_file=${work_file%.working}.result
      current_result_file=$result_file
      capture_path="$request_dir/$capture_name"

      if timeout --kill-after=1s 10s import -window root "$capture_path" >"$runner_tmpdir/import.log" 2>&1; then
        printf 'ok\n' >"$result_file"
      else
        local import_status=$?
        {
          printf 'error\n'
          printf 'request file: %s\n' "$work_file"
          printf 'capture path: %s\n' "$capture_path"
          printf 'DISPLAY: %s\n' "${DISPLAY:-unset}"
          printf 'import exit status: %s\n' "$import_status"
          printf 'import.log:\n'
          cat "$runner_tmpdir/import.log"
        } >"$result_file"
      fi
      cat "$runner_tmpdir/import.log"

      rm -f "$work_file"
      current_work_file=
      current_result_file=
    done
    shopt -u nullglob
    sleep 0.05
  done
}

start_host_capture_helper() {
  local capture_dir_unix
  capture_dir_unix=$(mktemp -d "$runner_tmpdir/host-capture.XXXXXX")
  export MIRX_TESTKIT_CAPTURE_DIR_UNIX=$capture_dir_unix
  export MIRX_TESTKIT_CAPTURE_DIR_WIN
  MIRX_TESTKIT_CAPTURE_DIR_WIN=$(unix_to_wine_path "$capture_dir_unix")

  host_capture_helper "$capture_dir_unix" >"$runner_tmpdir/host-capture-helper.log" 2>&1 &
  helper_pid=$!

  if ! import -window root "$capture_dir_unix/preflight.png" >"$runner_tmpdir/import.log" 2>&1; then
    die "host screenshot preflight failed: $(<"$runner_tmpdir/import.log")"
  fi
  rm -f "$capture_dir_unix/preflight.png"
}

cleanup() {
  if [[ -n "${helper_pid:-}" ]]; then
    kill "$helper_pid" >/dev/null 2>&1 || true
    wait "$helper_pid" 2>/dev/null || true
  fi

  if [[ "${started_xvfb:-0}" -eq 1 ]] && [[ -n "${xvfb_pid:-}" ]]; then
    kill "$xvfb_pid" >/dev/null 2>&1 || true
    wait "$xvfb_pid" 2>/dev/null || true
  fi

  if [[ -n "${runner_tmpdir:-}" ]]; then
    rm -rf "$runner_tmpdir"
  fi
}

main() {
  [[ $# -gt 0 ]] || die "expected a Windows test binary path"

  require_command wine
  require_command wineboot
  require_command Xvfb
  require_command xdpyinfo
  require_command import
  require_command timeout
  require_command flock

  runner_tmpdir=$(mktemp -d "${TMPDIR:-/var/tmp}/mirx-wine-runner.${runner_pid}.XXXXXX")
  trap cleanup EXIT
  ensure_xvfb
  ensure_wineprefix
  start_host_capture_helper

  wine "$@"
}

main "$@"
