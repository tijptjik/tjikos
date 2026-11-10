#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
bash scripts/check.sh
bash ../../packages/tjik-gtk/scripts/native.sh
export PKG_CONFIG_PATH="$(pwd)/../../packages/tjik-gtk/target/native/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
cargo build --release --locked --features gtk-ui
rm -rf dist
mkdir -p dist
cp -RL bin qml web systemd examples README.md dist/
cp ../../docs/icons/tasktjik-app.svg dist/tasktjik.svg
cp "${CARGO_TARGET_DIR:-target}/release/tasktjik-api" tasktjik-api
cp tasktjik-api dist/
cp "${CARGO_TARGET_DIR:-target}/release/tasktjik-gtk" dist/
mkdir -p dist/lib "${CARGO_TARGET_DIR:-target}/release/lib"
cp -L ../../packages/tjik-gtk/target/native/lib/libgtk4-layer-shell.so.0 dist/lib/
cp dist/lib/libgtk4-layer-shell.so.0 "${CARGO_TARGET_DIR:-target}/release/lib/"
cp ../../packages/tjik-gtk/target/layer-shell-source/LICENSE dist/lib/gtk4-layer-shell.LICENSE

python3 -B ../../scripts/package-notices.py bundle tasktjik "$PWD/dist"
