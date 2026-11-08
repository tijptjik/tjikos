#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
python3 -B tests/desktop_boundary.py
cargo fmt --manifest-path ../../packages/tjik-desktop/Cargo.toml --check
cargo fmt --check
for file in bin/* scripts/*.sh; do bash -n "$file"; done
