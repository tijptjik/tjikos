#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
mkdir -p target/tmp
export TMPDIR="${TMPDIR:-$PWD/target/tmp}"
cargo build --release --locked
mkdir -p dist/bin
cp -uRL --reflink=auto qml dist/
cp -uR --reflink=auto themes examples systemd docs dist/
cp bin/bartjik bin/bartjikctl dist/bin/
install -m 755 "${CARGO_TARGET_DIR:-target}/release/bartjik-core" dist/bin/bartjik-core
cp README.md dist/
rm -f dist/bartjik.py dist/qml/{audio_controls,audio_levels,network_controls,media_play,media_navigate,tray_attention,tray_focus}.py
rm -rf dist/bartjik_core dist/qml/__pycache__ dist/__pycache__
