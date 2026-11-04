#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
prefix="${WHATSOVER_INSTALL_PREFIX:-$HOME/.local}"
# The Rust client owns the public command; the former Baileys launcher is retired.
cargo build --manifest-path "$root/Cargo.toml" --locked --bin whatsover
target="$(cargo metadata --manifest-path "$root/Cargo.toml" --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
install -d -m 755 "$prefix/bin"
pending="$(mktemp "$prefix/bin/.whatsover.XXXXXXXX")"
trap 'rm -f -- "$pending"' EXIT
install -s -m 755 "$target/debug/whatsover" "$pending"
mv -f -- "$pending" "$prefix/bin/whatsover"
rm -f -- "$prefix/bin/whatsover-rust"
printf 'Installed %s/bin/whatsover. Next: whatsover status or whatsover login\n' "$prefix"
