#!/usr/bin/env bash
set -euo pipefail
mkdir -p dist
cp -R bin qml themes examples systemd dist/
cp notjik.py telegram_bridge.py telegram_recording.py telegram_media_render.py requirements-telegram.txt lock_watch.py comparison.py demo.py demo_cycle.py README.md dist/
chmod +x dist/bin/notjik dist/bin/notjikctl
cp scripts/install-telegram.sh dist/install-telegram.sh
