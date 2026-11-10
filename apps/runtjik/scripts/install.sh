#!/usr/bin/env bash
set -euo pipefail
root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
install_home="${RUNTJIK_INSTALL_HOME:-$HOME}"
config_root="${RUNTJIK_INSTALL_CONFIG:-${XDG_CONFIG_HOME:-$install_home/.config}}"
target="$install_home/.local/lib/runtjik"
test -x "$root_dir/dist/runtjik-core"
test -x "$root_dir/dist/runtjik-gtk"
if [[ "${TJIKUP_MANAGED_CONFIG:-0}" != 1 ]]; then
    TASKTJIK_INSTALL_HOME="$install_home" TASKTJIK_INSTALL_CONFIG="$config_root" bash "$root_dir/../tasktjik/scripts/install.sh"
fi

# Stop only this installed launcher before replacing a resident UI/backend pair.
if [[ "$install_home" == "$HOME" && -n "${WAYLAND_DISPLAY:-}" && -n "${XDG_RUNTIME_DIR:-}" && -x "$install_home/.local/bin/runtjik" ]]; then
    if ! RUNTJIK_HOME="$target" "$install_home/.local/bin/runtjik" quit >/dev/null 2>&1; then
        RUNTJIK_HOME="$target" "$install_home/.local/bin/runtjik" close >/dev/null 2>&1 || true
    fi
fi
if [[ "$install_home" == "$HOME" && -n "${XDG_RUNTIME_DIR:-}" && -x "$target/runtjik-gtk" ]]; then
    "$target/runtjik-gtk" quit
    "$target/runtjik-gtk" quit --demo
fi
install -d -m 755 "$install_home/.local/bin" "$target" "$config_root/runtjik"
for directory in bin assets helpers examples docs browser-extension browser-release lib; do
    staging="$(mktemp -d "$target/.install.XXXXXXXX")"
    cp -R "$root_dir/dist/$directory/." "$staging/"
    previous="$staging.previous"
    if [[ -e "$target/$directory" || -L "$target/$directory" ]]; then
        mv -- "$target/$directory" "$previous"
    fi
    mv -- "$staging" "$target/$directory"
    rm -rf -- "$previous"
done
install -m 644 "$root_dir/dist/README.md" "$target/"
# Atomic binary replacement leaves active recorder/browser workers on their inode.
new_binary="$(mktemp "$target/.runtjik-core.XXXXXXXX")"
install -m 755 "$root_dir/dist/runtjik-core" "$new_binary"
mv -f -- "$new_binary" "$target/runtjik-core"
new_gtk="$(mktemp "$target/.runtjik-gtk.XXXXXXXX")"
install -m 755 "$root_dir/dist/runtjik-gtk" "$new_gtk"
mv -f -- "$new_gtk" "$target/runtjik-gtk"
rm -f -- "$target"/{todo,browser,browser_host,image_viewer,runtjik,words,desktop_plugins,capture_worker,capture_countdown,capture_trash,control,compat_host}.py
"$target/runtjik-core" register-browser-host --runtjik-dir "$target" --manifest-dir "$install_home/.mozilla/native-messaging-hosts"
rm -rf -- "$target/__pycache__" "$target/qml"
install -m 755 "$root_dir/dist/bin/runtjik" "$install_home/.local/bin/runtjik"
bash "$root_dir/../../scripts/install-completions.sh" "$install_home/.local" runtjik
if [[ "${TJIKUP_MANAGED_CONFIG:-0}" != 1 && ! -e "$config_root/runtjik/runtjik.conf" ]]; then
    install -m 644 "$root_dir/dist/examples/runtjik.conf" "$config_root/runtjik/runtjik.conf"
fi
printf 'Installed Runtjik %s (warm after first open; no background service)\n' "$("$target/runtjik-core" version)"

install -m 755 "$root_dir/dist/bin/runtjik-image" "$install_home/.local/bin/runtjik-image"
install -d -m 755 "$install_home/.local/share/applications"
install -m 644 "$root_dir/dist/examples/runtjik-image.desktop" "$install_home/.local/share/applications/"

install -d -m 755 "$config_root/systemd/user"
install -m 644 "$root_dir/dist/systemd/"* "$config_root/systemd/user/"

python3 -B "$root_dir/../../scripts/package-notices.py" install "$root_dir/dist" "$target"
