#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

cd "$root_dir"
# The shipped exe: the release profile, without the `automation` feature, so
# without the automation harness (scripts/dll-gate.sh checks it is absent).
# Panic locations name the dependencies' sources by absolute path: these name
# them after /cargo and /mirx instead of the builder's directories, so the exe
# carries no home directory and builds alike anywhere. A --config array is
# appended to .cargo/config.toml's rustflags (RUSTFLAGS would replace them).
cargo build --release --package mirx-ui --target x86_64-pc-windows-gnu \
  --config "target.x86_64-pc-windows-gnu.rustflags = ['--remap-path-prefix=${CARGO_HOME:?}=/cargo', '--remap-path-prefix=$root_dir=/mirx']"
