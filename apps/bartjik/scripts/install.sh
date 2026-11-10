#!/usr/bin/env bash
set -euo pipefail
root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source_dir="$root_dir/dist"
install_home="${BARTJIK_INSTALL_HOME:-$HOME}"
target_dir="$install_home/.local/lib/bartjik"
config_dir="${BARTJIK_INSTALL_CONFIG:-${XDG_CONFIG_HOME:-$install_home/.config}}"
install_notices() {
    python3 -B "$root_dir/../../scripts/package-notices.py" install "$source_dir" "$target_dir"
}
if [[ "${1:-}" == --licenses-only ]]; then
    [[ $# == 1 ]] || { echo 'Usage: install.sh --licenses-only' >&2; exit 2; }
    install_notices
    exit 0
fi
test -x "$source_dir/bin/bartjik-core"
test -x "$source_dir/bin/bartjik-gtk"
test -f "$source_dir/qml/IconFonts.qml"
test -f "$source_dir/qml/fonts/MaterialSymbolsRounded.ttf"
install_notification_assets() {
    install -d -m 755 "$target_dir/lib/notification-cards/assets"
    cp -R "$source_dir/lib/notification-cards/assets/." "$target_dir/lib/notification-cards/assets/"
    install_notices
}
install_gtk() {
    install -d -m 755 "$target_dir/bin" "$target_dir/lib"
    install -m 755 "$source_dir/bin/bartjik-gtk-core" "$target_dir/bin/.bartjik-gtk-core.pending"
    mv -f "$target_dir/bin/.bartjik-gtk-core.pending" "$target_dir/bin/bartjik-gtk-core"
    install -m 755 "$source_dir/lib/libgtk4-layer-shell.so.0" "$target_dir/lib/.layer-shell.pending"
    mv -f "$target_dir/lib/.layer-shell.pending" "$target_dir/lib/libgtk4-layer-shell.so.0"
    install -m 644 "$source_dir/lib/gtk4-layer-shell.LICENSE" "$target_dir/lib/"
    install_notification_assets
    install -m 755 "$source_dir/bin/bartjik-gtk" "$target_dir/bin/.bartjik-gtk.pending"
    mv -f "$target_dir/bin/.bartjik-gtk.pending" "$target_dir/bin/bartjik-gtk"
}
if [[ "${1:-}" == --notification-assets-only ]]; then
    install_notification_assets
    printf 'Installed BarTjik notification artwork; binaries and services were not changed\n'
    exit 0
fi
if [[ "${1:-}" == --gtk-only ]]; then
    install_gtk
    install -d -m 755 "$install_home/.local/bin" "$target_dir/docs"
    install -m 755 "$source_dir/bin/bartjik" "$target_dir/bin/bartjik"
    install -m 755 "$source_dir/bin/bartjik" "$install_home/.local/bin/bartjik"
    cp -R "$source_dir/docs/." "$target_dir/docs/"
    install -m 644 "$source_dir/README.md" "$target_dir/"
    printf 'Installed opt-in Bartjik GTK; existing bar and services were not restarted\n'
    exit 0
fi
[[ $# == 0 ]] || { echo 'Usage: install.sh [--gtk-only|--notification-assets-only|--licenses-only]' >&2; exit 2; }
install -d -m 755 "$install_home/.local/bin" "$target_dir" "$config_dir/systemd/user"
cp -R "$source_dir"/qml "$source_dir"/themes "$source_dir"/examples "$source_dir"/docs "$source_dir"/hypr "$target_dir/"
install -m 644 "$source_dir/README.md" "$target_dir/"
install_gtk
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

# The scheduler remains independent of the bar and preserves the existing
# hyprsunset unit and nightlight on/off ExecCondition. Activation is explicit.
install -d -m 755 "$config_dir/systemd/user/hyprsunset.service.d" "$install_home/.local/share/applications"
install -m 644 "$source_dir/systemd/bartjik-nightlight.conf" "$config_dir/systemd/user/hyprsunset.service.d/bartjik.conf"
install -m 644 "$source_dir/applications/bartjik.desktop" "$install_home/.local/share/applications/bartjik.desktop"

# Keep-awake expiry must survive bar reloads and graphical-session restarts.
# The policy wrapper preserves existing game/desktop startup overrides.
install -d -m 755 "$config_dir/systemd/user/hypridle.service.d"
install -m 644 "$source_dir/systemd/bartjik-idle.conf" "$config_dir/systemd/user/hypridle.service.d/zz-bartjik-idle.conf"
install -m 644 "$source_dir/systemd/bartjik-idle.service" "$config_dir/systemd/user/bartjik-idle.service"
