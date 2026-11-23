#!/usr/bin/env bash
# Attributes the release exe's size to its symbols.
#
#   size-audit.sh [--top N] [--base <rev>]
#
# Builds mirx-ui release for x86_64-pc-windows-gnu with the release profile
# as it is (opt-level z, LTO, one codegen unit, panic abort) but unstripped,
# in a target dir of its own (never target/: the gates expect the stripped
# exe there). Writes target/size-audit/report/<rev>[-dirty].tsv (bytes, then
# the demangled .text symbol without its hash, equal names summed) and prints
# the stripped size, the section sizes, the biggest symbols and a per-crate
# sum. With --base it also builds <rev> in a detached worktree with its own
# target dir and prints what grew and what shrank.
#
# A measuring tool, not a gate: it fails only when a build fails.
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
target_triple=x86_64-pc-windows-gnu
nm_bin=${NM:-x86_64-w64-mingw32-nm}
objdump_bin=${OBJDUMP:-x86_64-w64-mingw32-objdump}
strip_bin=${STRIP:-x86_64-w64-mingw32-strip}

usage() {
  echo "usage: $0 [--top N] [--base <rev>]" >&2
  exit 2
}

top=30
base_rev=
while [[ $# -gt 0 ]]; do
  case "$1" in
    --top)
      [[ $# -ge 2 ]] || usage
      top=$2
      shift 2
      ;;
    --base)
      [[ $# -ge 2 ]] || usage
      base_rev=$2
      shift 2
      ;;
    *)
      usage
      ;;
  esac
done
[[ "$top" =~ ^[1-9][0-9]*$ ]] || usage

report_dir="$root_dir/target/size-audit/report"
mkdir -p "$report_dir"

base_src="$root_dir/target/size-audit-base/src"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/mirx-size-audit.XXXXXX")
cleanup() {
  rm -rf "$scratch"
  if [[ -d "$base_src" ]]; then
    git -C "$root_dir" worktree remove --force "$base_src"
  fi
}
trap cleanup EXIT

# build_exe <workspace dir> <target dir>: the unstripped release exe's path.
build_exe() {
  local src=$1 target=$2
  (
    cd "$src"
    CARGO_TARGET_DIR="$target" CARGO_PROFILE_RELEASE_STRIP=false \
      cargo build --release --package mirx-ui --target "$target_triple" >&2
  )
  echo "$target/$target_triple/release/mirx-ui.exe"
}

# section_size <exe> <section>: the section's size in bytes.
section_size() {
  "$objdump_bin" -h "$1" | gawk -v name="$2" '$2 == name { print strtonum("0x" $3) }'
}

stripped_size() {
  "$strip_bin" -o "$scratch/stripped.exe" "$1"
  stat -c %s "$scratch/stripped.exe"
}

# symbol_report <exe> <tsv>: bytes<TAB>symbol for every .text code symbol.
# COFF symbols carry no size, so a symbol's size is the distance to the next
# symbol's address (the last one ends with .text).
symbol_report() {
  local exe=$1 tsv=$2 text_start text_size
  read -r text_start text_size < <(
    "$objdump_bin" -h "$exe" | gawk '$2 == ".text" { print strtonum("0x" $4), strtonum("0x" $3) }'
  )
  "$nm_bin" -C -n --defined-only "$exe" | gawk -v start="$text_start" -v end="$((text_start + text_size))" '
    {
      address = strtonum("0x" $1)
      if (address < start || address >= end) next
      type = $2
      name = $0
      sub(/^[^ ]+ [^ ]+ /, "", name)
      sub(/::h[0-9a-f]{16}$/, "", name)
      if (count > 0 && address > addresses[count - 1]) {
        # Close the runs of symbols at the previous address.
        for (i = first; i < count; i++) ends[i] = address
        first = count
      }
      addresses[count] = address
      types[count] = type
      names[count] = name
      count++
    }
    END {
      counted = -1
      for (i = first; i < count; i++) ends[i] = end
      for (i = 0; i < count; i++) {
        if (types[i] != "t" && types[i] != "T") continue
        # Aliases at one address share its bytes: the first one takes them.
        if (addresses[i] == counted) continue
        counted = addresses[i]
        sizes[names[i]] += ends[i] - addresses[i]
      }
      for (name in sizes) printf "%d\t%s\n", sizes[name], name
    }
  ' | sort -t "$(printf '\t')" -k1,1nr -k2,2 >"$tsv"
}

# crate_sums <tsv>: bytes<TAB>crate, by the first `crate::` segment of each
# symbol; symbols without one (C, the runtime) are `(no path)`.
crate_sums() {
  gawk -F '\t' '
    {
      crate = "(no path)"
      if (match($2, /[A-Za-z_][A-Za-z0-9_]*::/)) crate = substr($2, RSTART, RLENGTH - 2)
      sums[crate] += $1
    }
    END { for (crate in sums) printf "%d\t%s\n", sums[crate], crate }
  ' "$1" | sort -t "$(printf '\t')" -k1,1nr -k2,2
}

# delta <base tsv> <current tsv>: change<TAB>name for every name that changed.
delta() {
  gawk -F '\t' '
    FNR == NR { base[$2] = $1; next }
    { current[$2] = $1 }
    END {
      for (name in current) if (current[name] != base[name]) printf "%d\t%s\n", current[name] - base[name], name
      for (name in base) if (!(name in current)) printf "%d\t%s\n", -base[name], name
    }
  ' "$1" "$2"
}

# first_n: the first $top lines, reading all input (no SIGPIPE upstream).
first_n() {
  gawk -v n="$top" 'NR <= n'
}

kib() {
  gawk -v bytes="$1" 'BEGIN { printf "%+.1f KiB", bytes / 1024 }'
}

# measure <label> <exe> <tsv>: prints the sizes; leaves the stripped size in
# $measured_stripped.
measure() {
  local label=$1 exe=$2 tsv=$3
  symbol_report "$exe" "$tsv"
  measured_stripped=$(stripped_size "$exe")
  printf '%s\n' "$label"
  printf '  stripped exe: %s bytes\n' "$measured_stripped"
  printf '  .text: %s bytes, .rdata: %s bytes\n' "$(section_size "$exe" .text)" "$(section_size "$exe" .rdata)"
  printf '  report: %s\n' "$tsv"
}

rev_name() {
  local src=$1 name
  name=$(git -C "$src" rev-parse --short HEAD)
  if [[ -n "$(git -C "$src" status --porcelain --untracked-files=no)" ]]; then
    name="$name-dirty"
  fi
  echo "$name"
}

current_exe=$(build_exe "$root_dir" "$root_dir/target/size-audit")
current_tsv="$report_dir/$(rev_name "$root_dir").tsv"
measure "current ($(rev_name "$root_dir"))" "$current_exe" "$current_tsv"
current_stripped=$measured_stripped

echo
echo "Crates are the first \`crate::\` segment of each symbol: a heuristic. A generic is"
echo "counted under whichever path comes first in its name, and LTO inlines callees into"
echo "their callers, so judge changes by totals and crate sums more than by one symbol."
echo
echo "top $top symbols (bytes):"
first_n <"$current_tsv"
echo
echo "by crate (bytes):"
crate_sums "$current_tsv" | first_n

if [[ -z "$base_rev" ]]; then
  exit 0
fi

base_commit=$(git -C "$root_dir" rev-parse --verify "$base_rev^{commit}")
if [[ -d "$base_src" ]]; then
  git -C "$root_dir" worktree remove --force "$base_src"
fi
mkdir -p "$(dirname "$base_src")"
# --force: a worktree a killed run left registered but deleted.
git -C "$root_dir" worktree add --force --detach "$base_src" "$base_commit" >&2
# .cargo/config.toml links against .nix-pthreads relative to the workspace;
# the devshell makes it only in the main checkout.
ln -s "$(readlink "$root_dir/.nix-pthreads")" "$base_src/.nix-pthreads"

base_exe=$(build_exe "$base_src" "$root_dir/target/size-audit-base/target")
base_tsv="$report_dir/$(git -C "$root_dir" rev-parse --short "$base_commit").tsv"
echo
measure "base ($base_rev)" "$base_exe" "$base_tsv"
base_stripped=$measured_stripped

stripped_delta=$((current_stripped - base_stripped))
echo
printf 'stripped exe: %s -> %s bytes (%+d, %s)\n' \
  "$base_stripped" "$current_stripped" "$stripped_delta" "$(kib "$stripped_delta")"
symbol_delta=$(gawk -F '\t' '{ sum += $1 } END { print sum + 0 }' <(delta "$base_tsv" "$current_tsv"))
printf 'code symbols: %+d bytes\n' "$symbol_delta"
echo
echo "by crate (change in bytes):"
delta <(crate_sums "$base_tsv") <(crate_sums "$current_tsv") | sort -t "$(printf '\t')" -k1,1nr -k2,2
echo
echo "top $top grown symbols (bytes):"
delta "$base_tsv" "$current_tsv" | gawk -F '\t' '$1 > 0' | sort -t "$(printf '\t')" -k1,1nr -k2,2 | first_n
echo
echo "top $top shrunk symbols (bytes):"
delta "$base_tsv" "$current_tsv" | gawk -F '\t' '$1 < 0' | sort -t "$(printf '\t')" -k1,1n -k2,2 | first_n
