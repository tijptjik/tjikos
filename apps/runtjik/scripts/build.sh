#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
bash scripts/check.sh
cargo build --release --locked
cp "${CARGO_TARGET_DIR:-target}/release/runtjik-core" runtjik-core
rm -rf -- dist
mkdir -p dist
cp -RL bin qml examples docs browser-extension systemd dist/
cp README.md dist/
rm -f dist/browser-extension/install-host.py
cp runtjik-core dist/
