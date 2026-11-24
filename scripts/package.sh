#!/usr/bin/env bash
# Builds the release zip people download: the shipped exe (cross-build.sh,
# checked by dll-gate.sh), the user's README, mirx's licence and the
# notices of what the exe is built from. Writes
# target/package/mirx-<commit>[-dirty]-windows-x64.zip (alone there) once
# it has passed its checks, and prints its path. The same commit and exe give the same zip, byte for byte.
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
target_triple=x86_64-pc-windows-gnu
exe_path="${CARGO_TARGET_DIR:-$root_dir/target}/$target_triple/release/mirx-ui.exe"
out_dir="$root_dir/target/package"
tmpdir=

cleanup() {
  if [[ -n "${tmpdir:-}" ]]; then
    rm -rf "$tmpdir"
  fi
}

cd "$root_dir"

"$root_dir/scripts/cross-build.sh" >&2
"$root_dir/scripts/dll-gate.sh" "$exe_path" >&2

commit=$(git rev-parse HEAD)
name="mirx-${commit:0:8}"
# Tracked files that differ from the commit: the zip is not that commit's.
if [[ -n $(git status --porcelain --untracked-files=no) ]]; then
  name+="-dirty"
fi
name+="-windows-x64"

tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/mirx-package.XXXXXX")
trap cleanup EXIT
stage="$tmpdir/stage/$name"
mkdir -p "$stage"

cp "$exe_path" "$stage/mirx-ui.exe"
cp packaging/README.txt "$stage/README.txt"
cp LICENSE "$stage/LICENSE.txt"
cp "${MIRX_MINGW_RUNTIME_NOTICE:?run in the devshell}" "$stage/COPYING.MinGW-w64-runtime.txt"
# Every crate's sources, so cargo-about reads their licence files offline.
cargo fetch --locked >&2
cargo about generate --frozen --manifest-path crates/mirx-ui/Cargo.toml \
  --config packaging/about.toml --output-file "$stage/THIRD-PARTY-LICENSES.txt" \
  packaging/third-party-licenses.hbs

# Reproducible: the commit's time on every entry (zip keeps local time,
# hence UTC), the same mode, entries in one order, no extra attributes.
commit_time=$(git log -1 --format=%ct HEAD)
chmod -R u=rwX,go=rX "$tmpdir/stage"
find "$tmpdir/stage" -exec touch --no-dereference --date="@$commit_time" {} +
zip_path="$tmpdir/$name.zip"
(
  cd "$tmpdir/stage"
  find "$name" | LC_ALL=C sort | TZ=UTC zip -X -q -9 "$zip_path" -@
)

# What the zip holds, as someone extracting it gets it: no file in it, and
# no file name, names mIRC.
mkdir "$tmpdir/unzipped"
unzip -q "$zip_path" -d "$tmpdir/unzipped"
unzip -Z1 "$zip_path" >"$tmpdir/listing.txt"
mapfile -t unzipped < <(find "$tmpdir/unzipped" -type f | LC_ALL=C sort)
"$root_dir/scripts/trademark-scan.sh" "$tmpdir/listing.txt" "${unzipped[@]}"

rm -rf "$out_dir"
mkdir -p "$out_dir"
mv "$zip_path" "$out_dir/"
echo "$out_dir/$name.zip"
