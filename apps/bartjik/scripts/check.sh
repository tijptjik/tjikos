#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
cargo fmt --check
cargo check --release --locked
BARTJIK_HOME="$PWD" cargo run --quiet --release --locked -- check-assets
bash -n bin/bartjik bin/bartjikctl scripts/build.sh scripts/install.sh
