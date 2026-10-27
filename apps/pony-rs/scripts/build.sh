#!/usr/bin/env bash
set -euo pipefail
app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cargo build --locked --release --manifest-path "$app_dir/Cargo.toml"
mkdir -p "$app_dir/dist"
install -m 755 "$app_dir/target/release/pony-rs" "$app_dir/dist/pony-rs"
