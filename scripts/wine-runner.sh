#!/usr/bin/env bash
set -euo pipefail

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

wait_for_display() {
  local socket_path=$1
  for _ in $(seq 1 50); do
    [[ -S "$socket_path" ]] && return 0
    sleep 0.1
  done
  return 1
}

ensure_xvfb() {
  if [[ -n "${DISPLAY:-}" ]] && [[ -S "$(display_socket_path "$DISPLAY")" ]]; then
    return 0
  fi

  local display=${MMATRIX_WINE_DISPLAY:-:99}
  local socket_path
  socket_path=$(display_socket_path "$display")
  export DISPLAY=$display

  if [[ -S "$socket_path" ]]; then
    return 0
  fi

  mkdir -p "${TMPDIR:-/tmp}"
  Xvfb "$DISPLAY" -screen 0 1024x768x24 >/tmp/mmatrix-xvfb.log 2>&1 &

  wait_for_display "$socket_path" || die "Xvfb did not create display socket for $DISPLAY"
}

ensure_wineprefix() {
  export WINEPREFIX="${WINEPREFIX:-$(pwd)/.wine-mmatrix-tests}"
  export WINEARCH="${WINEARCH:-win64}"
  export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:-mscoree,mshtml=}"
  export WINEDEBUG="${WINEDEBUG:--all}"

  mkdir -p "$WINEPREFIX"
  if [[ ! -f "$WINEPREFIX/system.reg" ]]; then
    wineboot -i >/dev/null 2>&1 || die "wineboot failed while initializing $WINEPREFIX"
  fi
}

main() {
  [[ $# -gt 0 ]] || die "expected a Windows test binary path"

  require_command wine
  require_command wineboot
  require_command Xvfb

  ensure_xvfb
  ensure_wineprefix

  exec wine "$@"
}

main "$@"
