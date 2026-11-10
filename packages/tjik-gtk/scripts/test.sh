#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
bash scripts/native.sh
native_lib="$PWD/target/native/lib"
export PKG_CONFIG_PATH="$native_lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$native_lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
cargo test --locked "$@"
