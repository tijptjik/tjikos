#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release --locked
cargo build --release --locked --manifest-path ../../packages/notjik-sync/Cargo.toml
mkdir -p dist/bin
install -m755 target/release/notjik-api ../../packages/notjik-sync/target/release/notjik-sync dist/bin/
cp -R systemd dist/
