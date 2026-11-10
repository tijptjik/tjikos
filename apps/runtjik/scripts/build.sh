#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
bash scripts/check.sh
bash ../../packages/tjik-gtk/scripts/native.sh
export PKG_CONFIG_PATH="$(pwd)/../../packages/tjik-gtk/target/native/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
cargo build --release --locked --features gtk-ui
mkdir -p "${CARGO_TARGET_DIR:-target}/release/lib" lib
cp -L ../../packages/tjik-gtk/target/native/lib/libgtk4-layer-shell.so.0 lib/
cp lib/libgtk4-layer-shell.so.0 "${CARGO_TARGET_DIR:-target}/release/lib/"
cp "${CARGO_TARGET_DIR:-target}/release/runtjik-core" runtjik-core
cp "${CARGO_TARGET_DIR:-target}/release/runtjik-gtk" runtjik-gtk
bash ../../packages/page-preview/scripts/build.sh
rm -rf -- dist
mkdir -p dist/helpers
cp -RL bin assets examples docs browser-extension browser-release systemd dist/
cp README.md dist/
rm -f dist/browser-extension/install-host.py
cp runtjik-core dist/
cp runtjik-gtk dist/
cp -R lib dist/
cp ../../packages/tjik-gtk/target/layer-shell-source/LICENSE dist/lib/gtk4-layer-shell.LICENSE
bash scripts/browser-extension.sh build

install -m 755 ../../packages/page-preview/dist/page-preview dist/helpers/page-preview

python3 -B ../../scripts/package-notices.py bundle runtjik "$PWD/dist"
