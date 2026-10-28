#!/usr/bin/env bash
set -euo pipefail
python3 scripts/check.py
mkdir -p dist
cp -R bin qml bartjik_core themes examples systemd docs dist/
cp bartjik.py README.md dist/
cp demo_cycle.py demo_tray.py DemoStage.qml demo_pointer.c demo_pointer.xml demo_gtk.c dist/
