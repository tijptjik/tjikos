#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
bash scripts/check.sh
cargo build --release --locked
rm -rf dist
mkdir -p dist
cp -RL bin qml web systemd examples README.md dist/
cp ../../docs/icons/tasktjik-app.svg dist/tasktjik.svg
cp "${CARGO_TARGET_DIR:-target}/release/tasktjik-api" tasktjik-api
cp tasktjik-api dist/

python3 -B ../../scripts/package-notices.py bundle tasktjik "$PWD/dist"
