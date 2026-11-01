#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
install_home="${GMAILTJIK_INSTALL_HOME:-$HOME}"
data_root="${GMAILTJIK_INSTALL_DATA:-${XDG_DATA_HOME:-$install_home/.local/share}}"
install -d "$install_home/.local/bin" "$data_root/applications" "$data_root/icons/hicolor/scalable/apps"
new_binary="$(mktemp "$install_home/.local/bin/.gmailtjik.XXXXXXXX")"
trap 'rm -f -- "$new_binary"' EXIT
install -m 755 "$root/dist/gmailtjik" "$new_binary"
mv -f -- "$new_binary" "$install_home/.local/bin/gmailtjik"
install -m 644 "$root/assets/gmailtjik-app.svg" "$data_root/icons/hicolor/scalable/apps/gmailtjik.svg"
# Absolute Exec also works when the desktop session omits ~/.local/bin from PATH.
python3 - "$root/examples/gmailtjik.desktop" "$data_root/applications/gmailtjik.desktop" "$install_home/.local/bin/gmailtjik" "$data_root/icons/hicolor/scalable/apps/gmailtjik.svg" <<'PY'
from pathlib import Path
import sys
import re
source, destination, binary, icon = sys.argv[1:]
escaped = binary.replace('\\', '\\\\').replace('"', '\\"').replace('`', '\\`').replace('$', '\\$').replace('%', '%%')
command = binary if re.fullmatch(r"[/A-Za-z0-9_.-]+", binary) else f'"{escaped}"'
Path(destination).write_text(Path(source).read_text().replace('Exec=gmailtjik', f'Exec={command}').replace('Icon=gmailtjik', f'Icon={icon}'))
PY
chmod 644 "$data_root/applications/gmailtjik.desktop"
if command -v update-desktop-database >/dev/null; then update-desktop-database "$data_root/applications"; fi
if [[ -f "$data_root/icons/hicolor/index.theme" ]] && command -v gtk-update-icon-cache >/dev/null; then
    gtk-update-icon-cache -f -t "$data_root/icons/hicolor"
fi
