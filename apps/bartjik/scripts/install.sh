#!/usr/bin/env bash
set -euo pipefail
root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source_dir="$root_dir/dist"
install_home="${BARTJIK_INSTALL_HOME:-$HOME}"
target_dir="$install_home/.local/lib/bartjik"
config_dir="${BARTJIK_INSTALL_CONFIG:-${XDG_CONFIG_HOME:-$install_home/.config}}"
test -x "$source_dir/bin/bartjik-core"
test -f "$source_dir/qml/IconFonts.qml"
test -f "$source_dir/qml/fonts/MaterialSymbolsRounded.ttf"
install -d -m 755 "$install_home/.local/bin" "$target_dir" "$config_dir/systemd/user"
cp -R "$source_dir"/qml "$source_dir"/themes "$source_dir"/examples "$source_dir"/docs "$target_dir/"
install -m 644 "$source_dir/README.md" "$target_dir/"
install -d -m 755 "$target_dir/bin"
install -m 755 "$source_dir/bin/bartjik-core" "$target_dir/bin/.bartjik-core.pending"
mv -f "$target_dir/bin/.bartjik-core.pending" "$target_dir/bin/bartjik-core"
install -m 755 "$source_dir/bin/bartjik" "$source_dir/bin/bartjikctl" "$target_dir/bin/"
rm -f "$target_dir/bartjik.py" "$target_dir"/qml/{audio_controls,audio_levels,network_controls,media_play,media_navigate,tray_attention,tray_focus}.py
rm -rf "$target_dir/bartjik_core" "$target_dir/__pycache__" "$target_dir/qml/__pycache__"
install -m 755 "$source_dir/bin/bartjik" "$source_dir/bin/bartjikctl" "$install_home/.local/bin/"
bash "$root_dir/../../scripts/install-completions.sh" "$install_home/.local" bartjik
if [[ "${TJIKUP_MANAGED_CONFIG:-0}" != 1 ]]; then
    install -m 644 "$source_dir/systemd/bartjik.service" "$config_dir/systemd/user/bartjik.service"
fi
