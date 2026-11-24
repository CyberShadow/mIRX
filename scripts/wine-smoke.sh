#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmpdir=
exe_path=

die() {
  echo "wine-smoke: $*" >&2
  exit 1
}

# shellcheck source=wine-prefix.sh
source "$root_dir/scripts/wine-prefix.sh"

cleanup() {
  if [[ -n "${tmpdir:-}" ]]; then
    rm -rf "$tmpdir"
  fi
}

cd "$root_dir"

tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/mirx-wine-smoke.XXXXXX")
trap cleanup EXIT

if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  export CARGO_TARGET_DIR="$tmpdir/cargo-target"
fi

exe_path=${1:-"$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release/mirx-ui.exe"}

if [[ $# -eq 0 ]] && [[ ! -f "$exe_path" ]]; then
  "$root_dir/scripts/cross-build.sh"
fi

ensure_wineprefix "$tmpdir/wineprefix"

wine_cmd=(wine)
if command -v xvfb-run >/dev/null 2>&1; then
  wine_cmd=(xvfb-run -a wine)
fi

# The shipped exe starts and closes by itself.
"${wine_cmd[@]}" "$exe_path" --auto-close || die "--auto-close exited $?"

# It has no automation harness: its help names only its own flags, and a
# harness flag is a usage error whose usage names none. Wine's X11 driver
# adds its own lines to stderr, so the usage is looked for among them.
release_usage="usage: mirx-ui [--auto-close|--help]"
harness_flags=(--automation --render-state --frame-size --self-test)
help=$("${wine_cmd[@]}" "$exe_path" --help) || die "--help exited $?"
help=${help//$'\r'/}
[[ $help == "$release_usage"$'\n'* ]] || die "--help printed: $help"
for flag in "${harness_flags[@]}"; do
  if [[ $help == *"$flag"* ]]; then
    die "--help names $flag: $help"
  fi
done
status=0
usage=$("${wine_cmd[@]}" "$exe_path" --automation 2>&1 >/dev/null) || status=$?
usage=${usage//$'\r'/}
[[ $status -eq 2 ]] || die "--automation exited $status, expected 2"
grep -qFx -- "$release_usage" <<<"$usage" || die "--automation printed no usage: $usage"

echo "wine-smoke ok: --auto-close; no automation harness"
