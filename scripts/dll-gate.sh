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
  "$root_dir/scripts/cross-build.sh"
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

# The shipped exe has no automation harness (the mirx-ui `automation`
# feature): nothing names it, in any case, and neither its launch flags nor
# its protocol's version field are there. ASCII, then UTF-16LE.
harness_words=('(?i:automation)' --render-state --self-test --frame-size protocol_version)
utf16le() { sed 's/./&\\x00/g' <<<"$1"; }
ascii_pattern=$(IFS='|'; echo "${harness_words[*]}")
utf16_pattern='(?i:a\x00u\x00t\x00o\x00m\x00a\x00t\x00i\x00o\x00n\x00)'
for word in "${harness_words[@]:1}"; do
  utf16_pattern+="|$(utf16le "$word")"
done
harness_hits=$(
  LC_ALL=C grep -a -o -b -P "[[:print:]]{0,40}(?:$ascii_pattern)[[:print:]]{0,40}" "$exe_path" || true
  LC_ALL=C grep -a -o -b -P "(?:[[:print:]]\x00){0,40}(?:$utf16_pattern)(?:[[:print:]]\x00){0,40}" "$exe_path" \
    | tr -d '\0' || true
)
if [[ -n $harness_hits ]]; then
  echo "dll-gate failed: $exe_path has the automation harness (offset:text):" >&2
  echo "$harness_hits" >&2
  exit 1
fi

# The exe names no directory of the machine that built it: cross-build.sh
# remaps the paths of the sources that panic locations name.
path_hits=$(
  for path in "$root_dir" "${CARGO_HOME:?}"; do
    LC_ALL=C grep -a -o -b -P "\\Q$path\\E[[:print:]]{0,60}" "$exe_path" | sed -n 1,5p || true
  done
)
if [[ -n $path_hits ]]; then
  echo "dll-gate failed: $exe_path names the build machine's directories (offset:text):" >&2
  echo "$path_hits" >&2
  exit 1
fi

printf 'dll-gate ok: %s; subsystem GUI; resources: icon, group icon, version, manifest; no "mIRC"; no automation harness; no build paths\n' "${imported_dlls[*]}"
