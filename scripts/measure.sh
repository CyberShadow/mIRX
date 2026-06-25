#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
exe_path="$root_dir/target/x86_64-pc-windows-gnu/release/mmatrix-ui.exe"
objdump_bin=${OBJDUMP:-x86_64-w64-mingw32-objdump}

cd "$root_dir"

"$root_dir/scripts/cross-build.sh"

size_bytes=$(stat -c %s "$exe_path")
size_human=$(numfmt --to=iec-i --suffix=B "$size_bytes")

printf 'exe: %s\n' "$exe_path"
printf 'size: %s bytes (%s)\n' "$size_bytes" "$size_human"
printf 'imports:\n'
"$objdump_bin" -p "$exe_path" | awk '/DLL Name:/ { print $3 }' | sort -u
