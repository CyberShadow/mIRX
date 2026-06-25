#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

cd "$root_dir"
cargo build --release --package mmatrix-ui --target x86_64-pc-windows-gnu
