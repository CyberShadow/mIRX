#!/usr/bin/env bash
# Runs the Windows UI test suites (automation_driver, mdi_child_reactivation,
# verify_duplicate_navigation_labels, scale) under Wine, split over parallel
# shards.
#
#   windows-ui-tests.sh [-- [--test-threads=1] [--exact] [filter ...]]
#
# Each shard has its own Wine prefix (so its own wineserver), its own Xvfb
# display and its own TMPDIR, and runs its tests one at a time. Filters are
# libtest name filters; they pick the tests when the suites are listed.
# MIRX_UI_TEST_SHARDS sets the shard count (default 12). Every shard's full
# output is kept in <target>/windows-ui-tests/shard-N.log.
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
suites=(automation_driver mdi_child_reactivation verify_duplicate_navigation_labels scale)
tmpdir=
# Running shards: pid -> shard number.
declare -A running=()

die() {
  echo "windows-ui-tests: $*" >&2
  exit 1
}

cleanup() {
  local pid prefix
  for pid in "${!running[@]}"; do
    kill -- "-$pid" >/dev/null 2>&1 || true
  done
  # wineservers outlive their last client for a few seconds; a killed run
  # must not leave them behind.
  for prefix in "$tmpdir/template-prefix" "$tmpdir"/shard-*/wineprefix; do
    if [[ -d "$prefix" ]]; then
      WINEPREFIX=$prefix wineserver -k >/dev/null 2>&1 || true
    fi
  done
  rm -rf "$tmpdir"
}

[[ -z "${WINEPREFIX:-}" ]] || die "WINEPREFIX is per shard; unset it"

list_args=(--list --format terse)
run_args=(--exact --test-threads=1)
if [[ $# -gt 0 ]]; then
  [[ $1 == -- ]] || die "usage: windows-ui-tests.sh [-- [--test-threads=1] [--exact] [filter ...]]"
  shift
  for arg in "$@"; do
    case $arg in
      # Each shard is always serial.
      --test-threads=1) ;;
      --exact) list_args+=(--exact) ;;
      -*) die "unsupported libtest flag: $arg" ;;
      *) list_args+=("$arg") ;;
    esac
  done
fi

shards=${MIRX_UI_TEST_SHARDS:-12}
[[ $shards =~ ^[1-9][0-9]*$ ]] || die "MIRX_UI_TEST_SHARDS must be a positive integer, got '$shards'"

cd "$root_dir"

# One run at a time per target dir: the runs share the shard logs (and the
# shards already use the machine's cores).
command -v flock >/dev/null 2>&1 || die "required command not found: flock"
log_dir="${CARGO_TARGET_DIR:-$root_dir/target}/windows-ui-tests"
mkdir -p "$log_dir"
exec 9>"$log_dir.lock"
flock 9
started=$SECONDS

# Build once; take each suite's test executable (and the app the tests launch,
# for the prefix warm-up) from cargo's JSON messages.
build_json=$(cargo test --target x86_64-pc-windows-gnu -p mirx-ui \
  "${suites[@]/#/--test=}" --no-run --message-format=json)
artifact_executable() {
  local kind=$1 name=$2
  grep -F "\"kind\":[\"$kind\"]" <<<"$build_json" | grep -F "\"name\":\"$name\"" |
    grep -o '"executable":"[^"]*"' | sed 's/^"executable":"//; s/"$//'
}
app_exe=$(artifact_executable bin mirx-ui)
[[ -f $app_exe ]] || die "cargo built no mirx-ui executable"
suite_exes=()
for suite in "${suites[@]}"; do
  exe=$(artifact_executable test "$suite")
  [[ -f $exe ]] || die "cargo built no executable for test suite $suite"
  suite_exes+=("$exe")
done

tmpdir=$(mktemp -d "${TMPDIR:-/var/tmp}/mirx-windows-ui-tests.XXXXXX")
set -m # every shard below runs in its own process group, for cleanup
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
rm -f "$log_dir"/*.log

# One template prefix, warmed by an app start, is copied to every shard, so
# no shard's first test pays for a fresh prefix. The suites are listed in it.
template_prefix="$tmpdir/template-prefix"
(
  export WINEPREFIX=$template_prefix
  unset DISPLAY MIRX_WINE_DISPLAY
  scripts/wine-runner.sh "$app_exe" --self-test >"$log_dir/warm-up.log" 2>&1 ||
    die "warming the Wine prefix failed; see $log_dir/warm-up.log"
  cd crates/mirx-ui
  listings=()
  for index in "${!suite_exes[@]}"; do
    "$root_dir/scripts/wine-runner.sh" "${suite_exes[$index]}" "${list_args[@]}" \
      >"$tmpdir/list-$index.out" 2>"$log_dir/list-$index.log" &
    listings+=($!)
  done
  for index in "${!listings[@]}"; do
    wait "${listings[$index]}" ||
      die "listing ${suites[$index]} failed; see $log_dir/list-$index.log"
  done
  wineserver -w
)

# Work items, in suite then name order: "suite-index name".
items=()
for index in "${!suite_exes[@]}"; do
  while IFS= read -r test_name; do
    items+=("$index $test_name")
  done < <(sed -n 's/: test\r\?$//p' "$tmpdir/list-$index.out" | sort)
done
[[ ${#items[@]} -gt 0 ]] || die "the filters select no test: ${list_args[*]:3}"
((shards <= ${#items[@]})) || shards=${#items[@]}

# Round-robin: neighbouring tests tend to cost alike, so this spreads the
# heavy families over the shards.
for ((shard = 0; shard < shards; shard++)); do
  mkdir -p "$tmpdir/shard-$shard/tmp"
done
for i in "${!items[@]}"; do
  item=${items[$i]}
  echo "${item#* }" >>"$tmpdir/shard-$((i % shards))/suite-${item%% *}.names"
done

# Test names passed to one test executable, in bytes. Wine's preloader
# leaves the main thread's Linux stack ~60 KiB, and the arguments and the
# environment (~33 KiB in the devshell) are on it: 16 KiB of names left too
# little stack, and the test executable died with SIGSEGV before its first
# test. So a shard runs its names in batches of at most this size.
max_names_bytes=4096

run_shard() {
  local shard=$1 dir="$tmpdir/shard-$1" status=0 index test_name batch batch_bytes
  export WINEPREFIX="$dir/wineprefix" TMPDIR="$dir/tmp"
  unset DISPLAY MIRX_WINE_DISPLAY
  cp -a "$template_prefix" "$WINEPREFIX"
  cd "$root_dir/crates/mirx-ui"
  # stdin is the names file being read.
  run_batch() {
    "$root_dir/scripts/wine-runner.sh" "${suite_exes[$index]}" "${run_args[@]}" "${batch[@]}" \
      </dev/null || status=1
  }
  for index in "${!suite_exes[@]}"; do
    [[ -f "$dir/suite-$index.names" ]] || continue
    batch=()
    batch_bytes=0
    while IFS= read -r test_name; do
      if ((${#batch[@]} > 0 && batch_bytes + ${#test_name} + 1 > max_names_bytes)); then
        run_batch
        batch=()
        batch_bytes=0
      fi
      batch+=("$test_name")
      batch_bytes=$((batch_bytes + ${#test_name} + 1))
    done <"$dir/suite-$index.names"
    run_batch
  done
  return "$status"
}

echo "windows-ui-tests: ${#items[@]} tests over $shards shards"
for ((shard = 0; shard < shards; shard++)); do
  run_shard "$shard" </dev/null >"$log_dir/shard-$shard.log" 2>&1 &
  running[$!]=$shard
done

failed_shards=()
while ((${#running[@]} > 0)); do
  status=0
  wait -n -p done_pid "${!running[@]}" || status=$?
  shard=${running[$done_pid]}
  unset "running[$done_pid]"
  ((status == 0)) || failed_shards+=("$shard")
  echo "windows-ui-tests: shard $shard done at $((SECONDS - started)) s (exit $status)"
done

passed=0
failed=0
for ((shard = 0; shard < shards; shard++)); do
  log="$log_dir/shard-$shard.log"
  while read -r shard_passed shard_failed; do
    passed=$((passed + shard_passed))
    failed=$((failed + shard_failed))
  done < <(sed -n 's/^test result: .*\. \([0-9]*\) passed; \([0-9]*\) failed;.*/\1 \2/p' "$log")
  grep '^test result:' "$log" | sed "s/^/shard $shard: /"
done
for shard in "${failed_shards[@]}"; do
  log="$log_dir/shard-$shard.log"
  echo
  echo "==== shard $shard failed ($log) ===="
  if grep -q '^failures:$' "$log"; then
    sed -n '/^failures:$/,/^test result:/p' "$log"
  else
    tail -n 40 "$log"
  fi
done

echo
echo "windows-ui-tests: $passed passed, $failed failed of ${#items[@]} tests over $shards shards in $((SECONDS - started)) s"
[[ ${#failed_shards[@]} -eq 0 ]] || exit 1
((passed == ${#items[@]})) || die "only $passed of ${#items[@]} tests passed"
