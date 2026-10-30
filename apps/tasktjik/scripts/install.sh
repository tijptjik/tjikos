#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
install_home="${TASKTJIK_INSTALL_HOME:-$HOME}"
config_root="${TASKTJIK_INSTALL_CONFIG:-${XDG_CONFIG_HOME:-$install_home/.config}}"
target="$install_home/.local/lib/tasktjik"
install -d "$target" "$install_home/.local/bin" "$config_root/systemd/user" "$install_home/.local/share/applications" "$install_home/.local/share/icons/hicolor/scalable/apps"
cp -R "$root/dist/qml" "$root/dist/web" "$target/"
new_binary="$(mktemp "$target/.tasktjik-api.XXXXXXXX")"
install -m 755 "$root/dist/tasktjik-api" "$new_binary"
mv -f -- "$new_binary" "$target/tasktjik-api"
rm -f -- "$target/api.py" "$target/client.py" "$target/tasktjik.py"
rm -rf -- "$target/__pycache__"
install -m 755 "$root/dist/bin/tasktjik" "$install_home/.local/bin/"
if [[ "${TJIKUP_MANAGED_CONFIG:-0}" != 1 ]]; then
    install -m 644 "$root/dist/systemd/"* "$config_root/systemd/user/"
fi
install -m 644 "$root/dist/examples/tasktjik.desktop" "$install_home/.local/share/applications/"
install -m 644 "$root/dist/tasktjik.svg" "$install_home/.local/share/icons/hicolor/scalable/apps/"
