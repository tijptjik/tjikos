#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
python3 -B scripts/check.py
rm -rf -- dist
mkdir -p dist
cp -R bin qml examples docs dist/
cp runtjik.py words.py desktop_plugins.py capture_worker.py capture_countdown.py capture_trash.py control.py README.md dist/
