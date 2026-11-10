#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
install_home="${TASKTJIK_INSTALL_HOME:-$HOME}"
config_root="${TASKTJIK_INSTALL_CONFIG:-${XDG_CONFIG_HOME:-$install_home/.config}}"
target="$install_home/.local/lib/tasktjik"
# Resource-only updates avoid replacing an independently developed native frontend.
if [[ "${1:-}" == --preview-helper-only ]]; then
    [[ $# == 1 ]] || { echo 'Usage: install.sh [--preview-helper-only]' >&2; exit 2; }
    test -f "$root/dist/qml/page-preview"
    install -d "$target/qml" "$target/licenses/page-preview"
    pending="$(mktemp "$target/qml/.page-preview.XXXXXXXX")"
    install -m 755 "$root/dist/qml/page-preview" "$pending"
    mv -f -- "$pending" "$target/qml/page-preview"
    install -m 644 "$root/dist/licenses/page-preview/"{LICENSE,NOTICE} "$target/licenses/page-preview/"
    # A resource-only update may target an older host; full installation retires
    # its legacy helper after updating both GTK and QML consumers.
    exit 0
fi
[[ $# == 0 ]] || { echo 'Usage: install.sh [--preview-helper-only]' >&2; exit 2; }
install -d "$target" "$install_home/.local/bin" "$config_root/systemd/user" "$install_home/.local/share/applications" "$install_home/.local/share/icons/hicolor/scalable/apps"
# Swap the packaged view/helper directory so a running preview keeps its inode.
staging="$(mktemp -d "$target/.qml-install.XXXXXXXX")"
cp -R "$root/dist/qml/." "$staging/"
previous="$staging.previous"
if [[ -e "$target/qml" || -L "$target/qml" ]]; then
    mv -- "$target/qml" "$previous"
fi
mv -- "$staging" "$target/qml"
rm -rf -- "$previous"
cp -R "$root/dist/web" "$target/"
install -d "$target/lib"
new_library="$(mktemp "$target/lib/.layer-shell.XXXXXXXX")"
install -m 755 "$root/dist/lib/libgtk4-layer-shell.so.0" "$new_library"
mv -f -- "$new_library" "$target/lib/libgtk4-layer-shell.so.0"
install -m 644 "$root/dist/lib/gtk4-layer-shell.LICENSE" "$target/lib/"
new_gtk="$(mktemp "$target/.tasktjik-gtk.XXXXXXXX")"
install -m 755 "$root/dist/tasktjik-gtk" "$new_gtk"
mv -f -- "$new_gtk" "$target/tasktjik-gtk"
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
# GTK's Wayland app ID must also resolve through icon-theme lookup, including
# panels whose desktop-entry list was cached before this installation.
for app_id in hk.type.tasktjik hk.type.tasktjik.demo; do
    install -m 644 "$root/dist/tasktjik.svg" "$install_home/.local/share/icons/hicolor/scalable/apps/$app_id.svg"
done
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$install_home/.local/share/icons/hicolor" >/dev/null
fi

# Use the unpadded app artwork even when an older icon theme is selected.
python3 - "$install_home/.local/share/applications/tasktjik.desktop" "$install_home/.local/share/icons/hicolor/scalable/apps/tasktjik.svg" <<'PYICON'
from pathlib import Path
import sys
path = Path(sys.argv[1])
path.write_text(path.read_text().replace("Icon=tasktjik", "Icon=" + sys.argv[2]))
PYICON

python3 -B "$root/../../scripts/package-notices.py" install "$root/dist" "$target"
