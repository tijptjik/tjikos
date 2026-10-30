#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
cargo fmt --check
cargo check --release --locked
bash -n bin/notjik bin/notjikctl scripts/build.sh scripts/install-telegram.sh
