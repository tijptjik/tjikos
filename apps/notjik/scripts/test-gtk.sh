#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
bash ../../packages/tjik-gtk/scripts/native.sh
native_lib="$(pwd)/../../packages/tjik-gtk/target/native/lib"
export PKG_CONFIG_PATH="$native_lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$native_lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
cargo test --release --locked --features gtk-ui
