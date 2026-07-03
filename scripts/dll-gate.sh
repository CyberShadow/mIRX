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
  tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/mmatrix-dll-gate.XXXXXX")
  trap cleanup EXIT
  export CARGO_TARGET_DIR="$tmpdir/cargo-target"
fi

exe_path=${1:-"$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release/mmatrix-ui.exe"}

cd "$root_dir"

if [[ $# -eq 0 ]] && [[ ! -f "$exe_path" ]]; then
  cargo build --release --package mmatrix-ui --target x86_64-pc-windows-gnu
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

printf 'dll-gate ok: %s\n' "${imported_dlls[*]}"
