#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
python3 -B scripts/check.py
cargo build --release --locked
cp "${CARGO_TARGET_DIR:-target}/release/runtjik-core" runtjik-core
rm -rf -- dist
mkdir -p dist
cp -RL bin qml examples docs browser-extension systemd dist/
cp todo.py browser.py browser_host.py image_viewer.py runtjik.py words.py desktop_plugins.py capture_worker.py capture_countdown.py capture_trash.py control.py compat_host.py README.md dist/
cp runtjik-core dist/
