#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
install_home="${TASKTJIK_INSTALL_HOME:-$HOME}"
config_root="${TASKTJIK_INSTALL_CONFIG:-${XDG_CONFIG_HOME:-$install_home/.config}}"
target="$install_home/.local/lib/tasktjik"
# Resource-only updates avoid replacing an independently developed native frontend.
if [[ "${1:-}" == --preview-helper-only ]]; then
    [[ $# == 1 ]] || { echo 'Usage: install.sh [--preview-helper-only]' >&2; exit 2; }
    test -f "$root/dist/qml/page_preview.py"
    install -d "$target/qml" "$target/licenses/page-preview"
    pending="$(mktemp "$target/qml/.page-preview.XXXXXXXX")"
    install -m 644 "$root/dist/qml/page_preview.py" "$pending"
    mv -f -- "$pending" "$target/qml/page_preview.py"
    install -m 644 "$root/dist/licenses/page-preview/"{LICENSE,NOTICE} "$target/licenses/page-preview/"
    exit 0
fi
[[ $# == 0 ]] || { echo 'Usage: install.sh [--preview-helper-only]' >&2; exit 2; }
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

# Use the unpadded app artwork even when an older icon theme is selected.
python3 - "$install_home/.local/share/applications/tasktjik.desktop" "$install_home/.local/share/icons/hicolor/scalable/apps/tasktjik.svg" <<'PYICON'
from pathlib import Path
import sys
path = Path(sys.argv[1])
path.write_text(path.read_text().replace("Icon=tasktjik", "Icon=" + sys.argv[2]))
PYICON

python3 -B "$root/../../scripts/package-notices.py" install "$root/dist" "$target"
