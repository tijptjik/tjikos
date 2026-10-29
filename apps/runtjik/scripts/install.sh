#!/usr/bin/env bash
set -euo pipefail
root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
install_home="${RUNTJIK_INSTALL_HOME:-$HOME}"
config_root="${RUNTJIK_INSTALL_CONFIG:-${XDG_CONFIG_HOME:-$install_home/.config}}"
target="$install_home/.local/lib/runtjik"
test -f "$root_dir/dist/runtjik.py"
# Stop only this installed launcher before replacing a resident UI/backend pair.
if [[ "$install_home" == "$HOME" && -n "${WAYLAND_DISPLAY:-}" && -n "${XDG_RUNTIME_DIR:-}" && -x "$install_home/.local/bin/runtjik" ]]; then
    if ! RUNTJIK_HOME="$target" "$install_home/.local/bin/runtjik" quit >/dev/null 2>&1; then
        RUNTJIK_HOME="$target" "$install_home/.local/bin/runtjik" close >/dev/null 2>&1 || true
    fi
fi
install -d -m 755 "$install_home/.local/bin" "$target" "$config_root/runtjik"
for directory in qml examples docs; do
    staging="$(mktemp -d "$target/.install.XXXXXXXX")"
    cp -R "$root_dir/dist/$directory/." "$staging/"
    previous="$staging.previous"
    if [[ -e "$target/$directory" || -L "$target/$directory" ]]; then
        mv -- "$target/$directory" "$previous"
    fi
    mv -- "$staging" "$target/$directory"
    rm -rf -- "$previous"
done
install -m 644 "$root_dir/dist/runtjik.py" "$root_dir/dist/control.py" "$root_dir/dist/README.md" "$target/"
install -m 755 "$root_dir/dist/bin/runtjik" "$install_home/.local/bin/runtjik"
bash "$root_dir/../../scripts/install-completions.sh" "$install_home/.local" runtjik
if [[ "${TJIKUP_MANAGED_CONFIG:-0}" != 1 && ! -e "$config_root/runtjik/runtjik.conf" ]]; then
    install -m 644 "$root_dir/dist/examples/runtjik.conf" "$config_root/runtjik/runtjik.conf"
fi
printf 'Installed Runtjik %s (warm after first open; no background service)\n' "$(python3 -B "$target/runtjik.py" version)"
