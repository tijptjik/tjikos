#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
cargo fmt --check
export PKG_CONFIG_PATH="$PWD/../../packages/tjik-gtk/target/native/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
cargo check --release --locked --features gtk-ui
BARTJIK_HOME="$PWD" cargo run --quiet --release --locked --bin bartjik-core -- check-assets
bash -n bin/bartjik bin/bartjikctl scripts/build.sh scripts/install.sh
