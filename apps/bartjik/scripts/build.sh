#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
mkdir -p target/tmp
export TMPDIR="${TMPDIR:-$PWD/target/tmp}"
bash ../../packages/tjik-gtk/scripts/native.sh
export PKG_CONFIG_PATH="$PWD/../../packages/tjik-gtk/target/native/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
cargo build --release --locked --features gtk-ui
mkdir -p dist/bin dist/lib/notification-cards/assets
cp -uR --reflink=auto ../notjik/assets/. dist/lib/notification-cards/assets/
rm -rf dist/lib/notification-cards/qml
cp -L ../../packages/tjik-gtk/target/native/lib/libgtk4-layer-shell.so.0 dist/lib/
cp ../../packages/tjik-gtk/target/layer-shell-source/LICENSE dist/lib/gtk4-layer-shell.LICENSE
install -m 755 "${CARGO_TARGET_DIR:-target}/release/bartjik-gtk" dist/bin/bartjik-gtk
rm -rf dist/assets dist/docs
cp -uR --reflink=auto assets dist/
cp -uR --reflink=auto themes examples systemd docs applications hypr dist/
cp bin/bartjik bin/bartjikctl dist/bin/
install -m 755 "${CARGO_TARGET_DIR:-target}/release/bartjik-core" dist/bin/bartjik-core
install -m 755 "${CARGO_TARGET_DIR:-target}/release/bartjik-core" dist/bin/bartjik-gtk-core
cp README.md dist/
rm -f dist/bartjik.py
rm -rf dist/bartjik_core dist/qml dist/__pycache__

python3 -B ../../scripts/package-notices.py bundle bartjik "$PWD/dist"
