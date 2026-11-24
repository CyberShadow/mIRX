#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

cd "$root_dir"
# The shipped exe: the release profile, without the `automation` feature, so
# without the automation harness (scripts/dll-gate.sh checks it is absent).
cargo build --release --package mirx-ui --target x86_64-pc-windows-gnu
