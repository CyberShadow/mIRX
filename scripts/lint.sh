#!/usr/bin/env bash
# The static checks: formatting, the clippy lanes (the host workspace, and
# on the Windows target mirx-ui without and with its `automation` feature
# and mirx-testkit), the GitHub workflows, and the shell scripts that are
# kept shellcheck-clean.
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
windows=(--target x86_64-pc-windows-gnu)

cd "$root_dir"

cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p mirx-ui --all-targets "${windows[@]}" -- -D warnings
cargo clippy -p mirx-ui --all-targets --features automation "${windows[@]}" -- -D warnings
cargo clippy -p mirx-testkit --all-targets "${windows[@]}" -- -D warnings
actionlint
shellcheck -x \
  scripts/cross-build.sh \
  scripts/dll-gate.sh \
  scripts/lint.sh \
  scripts/package.sh \
  scripts/trademark-scan.sh
