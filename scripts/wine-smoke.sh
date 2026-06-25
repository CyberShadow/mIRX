#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
exe_path=${1:-"$root_dir/target/x86_64-pc-windows-gnu/release/mmatrix-ui.exe"}

cd "$root_dir"

export WINEPREFIX="${WINEPREFIX:-$root_dir/.wine-mmatrix-smoke}"
export WINEARCH="${WINEARCH:-win64}"
export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:-mscoree,mshtml=}"

if command -v xvfb-run >/dev/null 2>&1; then
  exec xvfb-run -a wine "$exe_path" --self-test
fi

exec wine "$exe_path" --self-test
