#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
install_home="${TASKTJIK_INSTALL_HOME:-$HOME}"
config_root="${TASKTJIK_INSTALL_CONFIG:-${XDG_CONFIG_HOME:-$install_home/.config}}"
target="$install_home/.local/lib/tasktjik"
install -d "$target" "$install_home/.local/bin" "$config_root/systemd/user" "$install_home/.local/share/applications" "$install_home/.local/share/icons/hicolor/scalable/apps"
cp -R "$root/dist/qml" "$target/"
install -m 644 "$root/dist/tasktjik.py" "$target/"
install -m 755 "$root/dist/bin/tasktjik" "$install_home/.local/bin/"
if [[ "${TJIKUP_MANAGED_CONFIG:-0}" != 1 ]]; then
    install -m 644 "$root/dist/systemd/"* "$config_root/systemd/user/"
fi
install -m 644 "$root/dist/examples/tasktjik.desktop" "$install_home/.local/share/applications/"
install -m 644 "$root/dist/tasktjik.svg" "$install_home/.local/share/icons/hicolor/scalable/apps/"
