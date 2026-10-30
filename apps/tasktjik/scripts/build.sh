#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
bash scripts/check.sh
cargo build --release --locked
rm -rf dist
mkdir -p dist
cp -R bin qml web systemd examples README.md dist/
cp ../../docs/icons/tasktjik.svg dist/
cp "${CARGO_TARGET_DIR:-target}/release/tasktjik-api" tasktjik-api
cp tasktjik-api dist/
