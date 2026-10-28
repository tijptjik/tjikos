#!/usr/bin/env bash
set -euo pipefail
root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source_dir="$root_dir/dist"
install_home="${BARTJIK_INSTALL_HOME:-$HOME}"
target_dir="$install_home/.local/lib/bartjik"
config_dir="${BARTJIK_INSTALL_CONFIG:-${XDG_CONFIG_HOME:-$install_home/.config}}"
test -f "$source_dir/bartjik.py"
test -f "$source_dir/qml/IconFonts.qml"
test -f "$source_dir/qml/fonts/MaterialSymbolsRounded.ttf"
install -d -m 755 "$install_home/.local/bin" "$target_dir" "$config_dir/systemd/user"
cp -R "$source_dir"/qml "$source_dir"/bartjik_core "$source_dir"/themes "$source_dir"/examples "$source_dir"/docs "$target_dir/"
install -m 644 "$source_dir/bartjik.py" "$source_dir/README.md" "$target_dir/"
install -m 755 "$source_dir/bin/bartjik" "$source_dir/bin/bartjikctl" "$install_home/.local/bin/"
if [[ "${TJIKUP_MANAGED_CONFIG:-0}" != 1 ]]; then
    install -m 644 "$source_dir/systemd/bartjik.service" "$config_dir/systemd/user/bartjik.service"
fi
