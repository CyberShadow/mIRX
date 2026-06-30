#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
exe_path=${1:-"$root_dir/target/x86_64-pc-windows-gnu/release/mmatrix-ui.exe"}

die() {
  echo "wine-smoke: $*" >&2
  exit 1
}

ensure_wineprefix() {
  mkdir -p "$WINEPREFIX"
  if [[ ! -f "$WINEPREFIX/system.reg" ]]; then
    wineboot -i >/dev/null 2>&1 || die "wineboot failed while initializing $WINEPREFIX"
  fi
}

cd "$root_dir"

export WINEPREFIX="${WINEPREFIX:-$root_dir/.wine-mmatrix-smoke}"
export WINEARCH="${WINEARCH:-win64}"
export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:-mscoree,mshtml=}"

ensure_wineprefix

if command -v xvfb-run >/dev/null 2>&1; then
  exec xvfb-run -a wine "$exe_path" --self-test
fi

exec wine "$exe_path" --self-test
