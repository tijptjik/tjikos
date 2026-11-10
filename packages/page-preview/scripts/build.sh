#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
cargo build --release --locked
mkdir -p dist
install -m 755 "${CARGO_TARGET_DIR:-target}/release/page-preview" dist/page-preview
