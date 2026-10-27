#!/usr/bin/env bash
set -euo pipefail
root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if (( $# == 0 )); then
    set -- mailtjik notjik tjikup greetjik
fi
# Validate all targets before installing any of them.
for app in "$@"; do
    case "$app" in
        mailtjik|notjik|tjikup|bartjik|greetjik|runtjik) ;;
        *) printf 'Unknown app: %s (expected mailtjik, notjik, tjikup, bartjik, greetjik or runtjik)\n' "$app" >&2; exit 2 ;;
    esac
done
for app in "$@"; do
    case "$app" in
        runtjik)
            bash "$root_dir/apps/runtjik/scripts/install.sh"
            ;;
        bartjik)
            bash "$root_dir/apps/bartjik/scripts/install.sh"
            ;;
        greetjik)
            bash "$root_dir/apps/greetjik/scripts/install.sh"
            ;;
        tjikup)
            bash "$root_dir/apps/tjikup/scripts/install.sh"
            ;;
        mailtjik)
            install -d -m 755 "$HOME/.local/bin" "$HOME/.local/lib/mailtjik"
            install -m 755 "$root_dir/apps/mailtjik/dist/mailtjik" "$HOME/.local/bin/mailtjik"
            install -m 644 "$root_dir"/apps/mailtjik/dist/backend/*.py "$HOME/.local/lib/mailtjik/"
            ;;
        notjik)
            source_dir="$root_dir/apps/notjik/dist"
            target_dir="$HOME/.local/lib/notjik"
            install -d -m 755 "$HOME/.local/bin" "$target_dir" "$HOME/.config/systemd/user"
            cp -R "$source_dir"/qml "$source_dir"/themes "$source_dir"/examples "$target_dir/"
            install -m 644 "$source_dir/telegram_media_render.py" "$source_dir/telegram_recording.py" "$source_dir/telegram_bridge.py" "$source_dir/requirements-telegram.txt" "$source_dir/install-telegram.sh" "$source_dir/notjik.py" "$source_dir/lock_watch.py" "$source_dir/comparison.py" "$source_dir/demo.py" "$source_dir/demo_cycle.py" "$source_dir/README.md" "$target_dir/"
            install -m 755 "$source_dir/bin/notjik" "$source_dir/bin/notjikctl" "$HOME/.local/bin/"
            install -m 644 "$source_dir/systemd/notjik.service" "$HOME/.config/systemd/user/notjik.service"
            install -m 644 "$source_dir/systemd/notjik-telegram.service" "$HOME/.config/systemd/user/notjik-telegram.service"
            activation_dir="${XDG_DATA_HOME:-$HOME/.local/share}/dbus-1/services"
            install -d -m 755 "$activation_dir"
            printf '%s\n' '# Activate the notjik desktop notification service.' '[D-BUS Service]' 'Name=org.freedesktop.Notifications' \
                "Exec=$HOME/.local/bin/notjik" 'SystemdService=notjik.service' \
                > "$activation_dir/org.freedesktop.Notifications.service"
            chmod 644 "$activation_dir/org.freedesktop.Notifications.service"
            ;;
    esac
done
