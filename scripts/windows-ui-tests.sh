#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmpdir=

cleanup() {
  if [[ -n "${tmpdir:-}" ]]; then
    rm -rf "$tmpdir"
  fi
}

# Each run gets a fresh Wine prefix. Cargo builds incrementally into the
# normal target dir (or the caller's CARGO_TARGET_DIR).
if [[ -z "${WINEPREFIX:-}" ]]; then
  tmpdir=$(mktemp -d "${TMPDIR:-/var/tmp}/mirx-windows-ui-tests.XXXXXX")
  trap cleanup EXIT
fi

export WINEPREFIX="${WINEPREFIX:-$tmpdir/wineprefix}"
export WINEARCH="${WINEARCH:-win64}"
export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:-mscoree,mshtml=}"
# Timestamps show local time: pin it so the output is the same everywhere.
export TZ=UTC

command -v flock >/dev/null 2>&1 || {
  echo "windows-ui-tests: required command not found: flock" >&2
  exit 1
}
lock_file="${TMPDIR:-/var/tmp}/mirx-windows-ui-tests.lock"
exec 9>"$lock_file"
flock 9

cd "$root_dir"

cargo test --target x86_64-pc-windows-gnu -p mirx-ui --test automation_driver "$@"
cargo test --target x86_64-pc-windows-gnu -p mirx-ui --test mdi_child_reactivation "$@"
