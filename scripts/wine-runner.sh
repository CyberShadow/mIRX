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

  if [[ -n "${MMATRIX_WINE_DISPLAY:-}" ]]; then
    start_xvfb_explicit "$MMATRIX_WINE_DISPLAY"
  else
    start_xvfb_auto
  fi
}

ensure_wineprefix() {
  export WINEPREFIX="${WINEPREFIX:-$root_dir/.wine-mmatrix-tests}"
  export WINEARCH="${WINEARCH:-win64}"
  export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:-mscoree,mshtml=}"
  export WINEDEBUG="${WINEDEBUG:--all}"

  mkdir -p "$WINEPREFIX"
  if [[ ! -f "$WINEPREFIX/system.reg" ]]; then
    wineboot -i >/dev/null 2>&1 || die "wineboot failed while initializing $WINEPREFIX"
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

  while :; do
    shopt -s nullglob
    for request_file in "$request_dir"/*.request; do
      work_file=${request_file%.request}.working
      if ! mv "$request_file" "$work_file" 2>/dev/null; then
        continue
      fi

      capture_name=$(tr -d '\r\n' <"$work_file")
      result_file=${work_file%.working}.result
      capture_path="$request_dir/$capture_name"

      if import -window root "$capture_path" >"$runner_tmpdir/import.log" 2>&1; then
        printf 'ok\n' >"$result_file"
      else
        printf 'error\n' >"$result_file"
        cat "$runner_tmpdir/import.log" >>"$result_file"
      fi

      rm -f "$work_file"
    done
    shopt -u nullglob
    sleep 0.05
  done
}

start_host_capture_helper() {
  local capture_dir_unix
  capture_dir_unix=$(mktemp -d "$runner_tmpdir/host-capture.XXXXXX")
  export MMATRIX_TESTKIT_CAPTURE_DIR_UNIX=$capture_dir_unix
  export MMATRIX_TESTKIT_CAPTURE_DIR_WIN
  MMATRIX_TESTKIT_CAPTURE_DIR_WIN=$(unix_to_wine_path "$capture_dir_unix")

  host_capture_helper "$capture_dir_unix" &
  helper_pid=$!
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

  runner_tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/mmatrix-wine-runner.${runner_pid}.XXXXXX")
  trap cleanup EXIT
  ensure_xvfb
  ensure_wineprefix
  start_host_capture_helper

  wine "$@"
}

main "$@"
