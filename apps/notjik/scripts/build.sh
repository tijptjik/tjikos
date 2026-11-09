#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
bash ../../packages/tjik-gtk/scripts/native.sh
export PKG_CONFIG_PATH="$(pwd)/../../packages/tjik-gtk/target/native/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
cargo build --release --locked --features gtk-ui
cargo build --release --locked --manifest-path ../../packages/notjik-sync/Cargo.toml
mkdir -p dist/bin dist/lib
cp -L ../../packages/tjik-gtk/target/native/lib/libgtk4-layer-shell.so.0 dist/lib/
cp ../../packages/tjik-gtk/target/layer-shell-source/LICENSE dist/lib/gtk4-layer-shell.LICENSE
install -m 755 "${CARGO_TARGET_DIR:-target}/release/notjik-gtk" dist/bin/notjik-gtk
install -m755 ../../packages/notjik-sync/target/release/notjik-sync dist/bin/
cp ../notjik-service/systemd/notjik-sync.service systemd/
rm -rf dist/qml
rm -f dist/examples/DemoStage.qml dist/examples/Issue.qml dist/examples/demo-patches.json dist/examples/reference-artwork.js
cp -uR --reflink=auto assets themes examples systemd dist/
cp bin/notjik bin/notjikctl dist/bin/
install -m 755 "${CARGO_TARGET_DIR:-target}/release/notjik-core" dist/bin/notjik-core
mkdir -p dist/docs
cp docs/*.md dist/docs/
cp README.md dist/
cp scripts/install-telegram.sh dist/install-telegram.sh
# Pre-release: the distribution contains no interpreter runtime or fallbacks.
rm -f dist/*.py dist/requirements-telegram.txt
rm -rf dist/__pycache__
