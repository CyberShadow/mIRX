#!/usr/bin/env bash
# Fails if any given file names "mIRC" (a trademark): nothing shipped may,
# in any case, visible or not. "mirx" is the product's name. Each file is
# read as bytes, as ASCII, then as UTF-16LE at any offset; every hit is
# printed as file:offset:text.
set -euo pipefail

if [[ $# -eq 0 ]]; then
  echo "usage: trademark-scan.sh FILE..." >&2
  exit 2
fi

hits=$(
  for file in "$@"; do
    [[ -f $file ]] || { echo "trademark-scan: no such file: $file" >&2; exit 2; }
    {
      LC_ALL=C grep -a -o -b -i -P '[[:print:]]{0,40}mirc(?!x)[[:print:]]{0,40}' "$file" || true
      LC_ALL=C grep -a -o -b -i -P '(?:[[:print:]]\x00){0,40}m\x00i\x00r\x00c\x00(?!x\x00)(?:[[:print:]]\x00){0,40}' "$file" \
        | tr -d '\0' || true
    } | awk -v file="$file" '{ print file ":" $0 }'
  done
)
if [[ -n $hits ]]; then
  echo "trademark-scan failed: mIRC is named (file:offset:text):" >&2
  echo "$hits" >&2
  exit 1
fi
