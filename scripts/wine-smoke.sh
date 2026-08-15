#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmpdir=
exe_path=

die() {
  echo "wine-smoke: $*" >&2
  exit 1
}

cleanup() {
  if [[ -n "${tmpdir:-}" ]]; then
    rm -rf "$tmpdir"
  fi
}

ensure_wineprefix() {
  export WINEPREFIX="${WINEPREFIX:-$tmpdir/wineprefix}"
  export WINEARCH="${WINEARCH:-win64}"
  export WINEDLLOVERRIDES=mscoree,mshtml=

  mkdir -p "$WINEPREFIX"
  if [[ ! -f "$WINEPREFIX/system.reg" ]]; then
    wineboot -i >/dev/null 2>&1 || die "wineboot failed while initializing $WINEPREFIX"
  fi
}

cd "$root_dir"

tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/mirx-wine-smoke.XXXXXX")
trap cleanup EXIT

if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  export CARGO_TARGET_DIR="$tmpdir/cargo-target"
fi

exe_path=${1:-"$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release/mirx-ui.exe"}

ensure_wineprefix

if command -v xvfb-run >/dev/null 2>&1; then
  xvfb-run -a wine "$exe_path" --self-test
  exit $?
fi

wine "$exe_path" --self-test
