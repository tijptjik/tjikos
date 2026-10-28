#!/usr/bin/env bash
set -euo pipefail
app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
prefix="${1:-$HOME/.local}"
bash "$app_dir/scripts/fetch-release.sh"
# Refuse to replace a working legacy runner underneath an incompatible workflow.
config="${TJIKUP_INSTALL_CONFIG:-${XDG_CONFIG_HOME:-$HOME/.config}/tjikup/config.json}"
if [[ -f "$config" ]]; then
    "$app_dir/dist/tjikup" --config "$config" --check
fi
install -d -m 755 "$prefix/bin" "$prefix/share/icons"
# Replace atomically; an in-progress run keeps its loaded executable.
staged_binary="$(mktemp "$prefix/bin/.tjikup.XXXXXX")"
trap 'rm -f "$staged_binary"' EXIT
install -m 755 "$app_dir/dist/tjikup" "$staged_binary"
mv -f "$staged_binary" "$prefix/bin/tjikup"
install -m 644 "$app_dir/assets/tjikup.png" "$prefix/share/icons/tjikup.png"
# Named icon for notifications arriving through an SSH terminal.
install -d -m 755 "$prefix/share/icons/hicolor/256x256/apps"
ln -sfn ../../../tjikup.png "$prefix/share/icons/hicolor/256x256/apps/tjikup-notification.png"

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    if ! icon_cache_output="$(gtk-update-icon-cache --force --ignore-theme-index "$prefix/share/icons/hicolor" 2>&1)"; then
        printf '%s\n' "$icon_cache_output" >&2
        exit 1
    fi
fi
