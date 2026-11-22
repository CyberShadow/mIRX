#!/usr/bin/env bash
# Runs the live lane: the release mirx-ui.exe, driven through its GUI by the
# `live` test suite (crates/mirx-ui/tests/live.rs) under Wine, against a
# real Conduit homeserver, in a NixOS VM (nix/live-lane.nix).
#
#   live-lane.sh [-- <filter>...]
#
# Filters are libtest name filters. The Windows binaries are built here
# with the dev shell's toolchain and added to the Nix store; the VM test is
# built by the Nix daemon, which needs the kvm and nixos-test features.
# Every run runs the tests: a nonce makes each a new derivation.
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
target_dir=${CARGO_TARGET_DIR:-$root_dir/target}
staging=

die() {
  echo "live-lane: $*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

cleanup() {
  if [[ -n "$staging" ]]; then
    rm -rf "$staging"
  fi
}

filters=()
if [[ $# -gt 0 ]]; then
  [[ $1 == -- ]] || die "usage: live-lane.sh [-- <filter>...]"
  shift
  for arg in "$@"; do
    # Filters go into a Nix expression: nothing that needs escaping.
    [[ $arg =~ ^[A-Za-z0-9_:]+$ ]] || die "a filter is [A-Za-z0-9_:]+, got '$arg'"
    filters+=("$arg")
  done
fi

require_command nix
require_command cargo
require_command flock
require_command x86_64-w64-mingw32-strip

cd "$root_dir"

# One run at a time per target dir: the runs share the release build.
mkdir -p "$target_dir"
exec 9>"$target_dir/live-lane.lock"
flock 9
started=$SECONDS

scripts/cross-build.sh
app_exe="$target_dir/x86_64-pc-windows-gnu/release/mirx-ui.exe"
[[ -f $app_exe ]] || die "cross-build.sh built no $app_exe"
build_json=$(cargo test --target x86_64-pc-windows-gnu -p mirx-ui --test live --no-run \
  --message-format=json)
live_exe=$(grep -F '"kind":["test"]' <<<"$build_json" | grep -F '"name":"live"' |
  grep -o '"executable":"[^"]*"' | sed 's/^"executable":"//; s/"$//')
[[ -f $live_exe ]] || die "cargo built no executable for test suite live"

staging=$(mktemp -d "${TMPDIR:-/var/tmp}/mirx-live-lane.XXXXXX")
trap cleanup EXIT
cp "$app_exe" "$staging/mirx-ui.exe"
# The debug test exe is mostly debug info; the symbols stay, for backtraces.
x86_64-w64-mingw32-strip --strip-debug -o "$staging/live.exe" "$live_exe"
bins=$(nix store add --name mirx-live-bins "$staging")

nix_filters=
for filter in "${filters[@]}"; do
  nix_filters+=" \"$filter\""
done
status=0
# git+file: a bare path is a path: flake, which copies the whole tree
# (target/ too) into the store; git+file takes the tracked files only.
nix build -L --no-link --impure --expr "
  (builtins.getFlake \"git+file://$root_dir\").legacyPackages.x86_64-linux.liveLane {
    bins = builtins.storePath \"$bins\";
    filters = [$nix_filters ];
    nonce = \"$(date +%s%N)\";
  }" || status=$?

echo "live-lane: exit $status in $((SECONDS - started)) s"
exit "$status"
