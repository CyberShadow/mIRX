#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
objdump_bin=${OBJDUMP:-x86_64-w64-mingw32-objdump}
tmpdir=

cleanup() {
  if [[ -n "${tmpdir:-}" ]]; then
    rm -rf "$tmpdir"
  fi
}

if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/mirx-dll-gate.XXXXXX")
  trap cleanup EXIT
  export CARGO_TARGET_DIR="$tmpdir/cargo-target"
fi

exe_path=${1:-"$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release/mirx-ui.exe"}

cd "$root_dir"

if [[ $# -eq 0 ]] && [[ ! -f "$exe_path" ]]; then
  cargo build --release --package mirx-ui --target x86_64-pc-windows-gnu
fi

declare -a allowlist=(
  advapi32.dll
  api-ms-win-core-synch-l1-2-0.dll
  bcrypt.dll
  bcryptprimitives.dll
  comctl32.dll
  comdlg32.dll
  crypt32.dll
  gdi32.dll
  gdiplus.dll
  kernel32.dll
  msvcrt.dll
  ntdll.dll
  ole32.dll
  oleaut32.dll
  rpcrt4.dll
  secur32.dll
  shell32.dll
  user32.dll
  userenv.dll
  ws2_32.dll
)

mapfile -t imported_dlls < <("$objdump_bin" -p "$exe_path" | awk '/DLL Name:/ { print tolower($3) }' | sort -u)

if [ "${#imported_dlls[@]}" -eq 0 ]; then
  echo "dll-gate failed: no imported DLLs found in $exe_path" >&2
  exit 1
fi

for dll in "${imported_dlls[@]}"; do
  allowed=false

  for candidate in "${allowlist[@]}"; do
    if [ "$dll" = "$candidate" ]; then
      allowed=true
      break
    fi
  done

  if [ "$allowed" = false ]; then
    echo "dll-gate failed: disallowed DLL import $dll" >&2
    exit 1
  fi
done

headers=$("$objdump_bin" -p "$exe_path")

# A GUI-subsystem program: no console window opens beside it.
if ! grep -E '^Subsystem' <<<"$headers" | grep -qF '(Windows GUI)'; then
  echo "dll-gate failed: $exe_path is not a Windows GUI subsystem program" >&2
  exit 1
fi

# The resources build.rs links in. Type entries are the least-indented
# `Entry:` lines of the resource tree, directly under its root Type Table.
resource_types=$(awk '
  /^The \.rsrc Resource Directory section:/ { in_rsrc = 1; next }
  in_rsrc && /^$/ { in_rsrc = 0 }
  in_rsrc && /^[0-9a-f]+   Entry: ID: / { sub(/,$/, "", $4); print $4 }
' <<<"$headers" | sort | paste -sd ' ')
expected_types="0x000003 0x00000e 0x000010 0x000018"
if [ "$resource_types" != "$expected_types" ]; then
  echo "dll-gate failed: resource types are '$resource_types', expected '$expected_types' (icon, group icon, version, manifest)" >&2
  exit 1
fi

# The manifest asks for themed common controls (comctl32 v6).
if [ "$(grep -a -c 'Microsoft.Windows.Common-Controls' "$exe_path")" -lt 1 ]; then
  echo "dll-gate failed: the manifest's Common-Controls 6.0 dependency is missing" >&2
  exit 1
fi

# The manifest declares Windows 10/11.
if [ "$(grep -a -c '8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a' "$exe_path")" -lt 1 ]; then
  echo "dll-gate failed: the manifest's Windows 10/11 supportedOS is missing" >&2
  exit 1
fi

# The manifest makes mirx system-DPI-aware.
if ! grep -a -q -E '<dpiAware[^>]*>true</dpiAware>' "$exe_path"; then
  echo "dll-gate failed: the manifest's system DPI awareness is missing" >&2
  exit 1
fi

# Nothing shipped names "mIRC" (a trademark), in any case, visible or not:
# strings, resources, panic messages and the source paths panic locations
# embed. "mirx" is the product's name. ASCII, then UTF-16LE at any offset.
trademark_hits=$(
  LC_ALL=C grep -a -o -b -i -P '[[:print:]]{0,40}mirc(?!x)[[:print:]]{0,40}' "$exe_path" || true
  LC_ALL=C grep -a -o -b -i -P '(?:[[:print:]]\x00){0,40}m\x00i\x00r\x00c\x00(?!x\x00)(?:[[:print:]]\x00){0,40}' "$exe_path" \
    | tr -d '\0' || true
)
if [[ -n $trademark_hits ]]; then
  echo "dll-gate failed: $exe_path names mIRC (offset:text):" >&2
  echo "$trademark_hits" >&2
  exit 1
fi

printf 'dll-gate ok: %s; subsystem GUI; resources: icon, group icon, version, manifest; no "mIRC"\n' "${imported_dlls[*]}"
