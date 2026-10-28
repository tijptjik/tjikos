#!/usr/bin/env bash
set -euo pipefail
python3 scripts/check.py
rm -rf -- dist
mkdir -p dist
cp -R bin qml bartjik_core themes examples systemd docs dist/
cp bartjik.py README.md dist/
