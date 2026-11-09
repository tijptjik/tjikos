#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
cargo fmt --check
bash ../../packages/tjik-gtk/scripts/native.sh
export PKG_CONFIG_PATH="$(pwd)/../../packages/tjik-gtk/target/native/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
cargo check --release --locked --features gtk-ui
bash -n bin/notjik bin/notjikctl scripts/build.sh scripts/install-telegram.sh scripts/test-gtk.sh
