#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
python3 -B scripts/check.py
cargo build --release --locked
rm -rf dist
mkdir -p dist
cp -R bin qml web systemd examples tasktjik.py api.py client.py README.md dist/
cp ../../docs/icons/tasktjik.svg dist/
cp target/release/tasktjik-api dist/
