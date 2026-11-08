#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
python3 -B tests/desktop_boundary.py
cargo fmt --manifest-path ../../packages/tjik-desktop/Cargo.toml --check
cargo fmt --check
cargo fmt --manifest-path ../../packages/tjik-gtk/Cargo.toml --check
cargo fmt --manifest-path ../../packages/tasktjik-gtk/Cargo.toml --check
rustfmt --edition 2021 --check src/gtk/main.rs
for file in bin/* scripts/*.sh; do
    if head -1 "$file" | grep -q python3; then
        python3 -c 'import ast,sys; ast.parse(open(sys.argv[1]).read())' "$file"
    else
        bash -n "$file"
    fi
done
