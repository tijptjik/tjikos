#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
sha256sum Cargo.toml Cargo.lock src/*.rs scripts/*.sh | sha256sum | cut -d ' ' -f 1
