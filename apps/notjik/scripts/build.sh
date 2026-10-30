#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
cargo build --release --locked
mkdir -p dist/bin
cp -uRL --reflink=auto qml dist/
cp -uR --reflink=auto themes examples systemd dist/
cp bin/notjik bin/notjikctl dist/bin/
install -m 755 "${CARGO_TARGET_DIR:-target}/release/notjik-core" dist/bin/notjik-core
cp README.md dist/
cp scripts/install-telegram.sh dist/install-telegram.sh
# Pre-release: the distribution contains no interpreter runtime or fallbacks.
rm -f dist/*.py dist/requirements-telegram.txt
rm -rf dist/__pycache__
