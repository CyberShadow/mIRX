#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmpdir=

cleanup() {
  if [[ -n "${tmpdir:-}" ]]; then
    rm -rf "$tmpdir"
  fi
}

if [[ -z "${CARGO_TARGET_DIR:-}" ]] || [[ -z "${WINEPREFIX:-}" ]]; then
  tmpdir=$(mktemp -d "${TMPDIR:-/var/tmp}/mmatrix-windows-ui-tests.XXXXXX")
  trap cleanup EXIT
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$tmpdir/cargo-target}"
export WINEPREFIX="${WINEPREFIX:-$tmpdir/wineprefix}"
export WINEARCH="${WINEARCH:-win64}"
export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:-mscoree,mshtml=}"

command -v flock >/dev/null 2>&1 || {
  echo "windows-ui-tests: required command not found: flock" >&2
  exit 1
}
lock_file="${TMPDIR:-/var/tmp}/mmatrix-windows-ui-tests.lock"
exec 9>"$lock_file"
flock 9

cd "$root_dir"

cargo test --target x86_64-pc-windows-gnu -p mmatrix-ui --test automation_driver "$@"
cargo test --target x86_64-pc-windows-gnu -p mmatrix-ui --test mdi_child_reactivation "$@"
