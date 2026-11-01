#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
cargo build --release --locked
cargo build --release --locked --manifest-path ../../packages/notjik-sync/Cargo.toml
mkdir -p dist/bin
install -m755 ../../packages/notjik-sync/target/release/notjik-sync dist/bin/
cp ../notjik-service/systemd/notjik-sync.service systemd/
cp -uRL --reflink=auto qml dist/
cp -uR --reflink=auto themes examples systemd dist/
cp bin/notjik bin/notjikctl dist/bin/
install -m 755 "${CARGO_TARGET_DIR:-target}/release/notjik-core" dist/bin/notjik-core
cp README.md dist/
cp scripts/install-telegram.sh dist/install-telegram.sh
# Pre-release: the distribution contains no interpreter runtime or fallbacks.
rm -f dist/*.py dist/requirements-telegram.txt
rm -rf dist/__pycache__
