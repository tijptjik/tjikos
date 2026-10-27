#!/usr/bin/env bash
set -euo pipefail
app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
install -d -m 755 "$HOME/.local/bin" "$HOME/.local/share/licenses/pony-rs"
# Rename atomically so concurrently starting shells see either complete binary.
install -m 755 "$app_dir/dist/pony-rs" "$HOME/.local/bin/.pony-rs.new"
mv -f -- "$HOME/.local/bin/.pony-rs.new" "$HOME/.local/bin/pony-rs"
install -m 644 "$app_dir/LICENSE" "$app_dir/assets/UPSTREAM-LICENSE" "$app_dir/assets/CREDITS" "$app_dir/assets/UPSTREAM.md" "$HOME/.local/share/licenses/pony-rs/"
