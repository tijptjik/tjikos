#!/usr/bin/env bash
set -euo pipefail
root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if (( $# == 0 )); then
    set -- mailtjik notjik tjikup synctjik greetjik
fi
# Validate all targets before installing any of them.
for app in "$@"; do
    case "$app" in
        mailtjik|notjik|tjikup|synctjik|bartjik|greetjik|runtjik|tasktjik) ;;
        *) printf 'Unknown app: %s (expected mailtjik, notjik, tjikup, synctjik, bartjik, greetjik, runtjik or tasktjik)\n' "$app" >&2; exit 2 ;;
    esac
done
for app in "$@"; do
    case "$app" in
        tasktjik)
            bash "$root_dir/apps/tasktjik/scripts/install.sh"
            ;;
        runtjik)
            bash "$root_dir/apps/runtjik/scripts/install.sh"
            ;;
        bartjik)
            bash "$root_dir/apps/bartjik/scripts/install.sh"
            ;;
        greetjik)
            bash "$root_dir/apps/greetjik/scripts/install.sh"
            ;;
        synctjik)
            bash "$root_dir/apps/synctjik/scripts/install.sh"
            ;;
        tjikup)
            bash "$root_dir/apps/tjikup/scripts/install.sh"
            ;;
        mailtjik)
            install -d -m 755 "$HOME/.local/bin" "$HOME/.local/lib/mailtjik"
            install -m 755 "$root_dir/apps/mailtjik/dist/mailtjik" "$HOME/.local/bin/mailtjik"
            install -m 644 "$root_dir"/apps/mailtjik/dist/backend/*.py "$HOME/.local/lib/mailtjik/"
            bash "$root_dir/scripts/install-completions.sh" "$HOME/.local" mailtjik
            ;;
        notjik)
            if [[ "${TJIKUP_MANAGED_CONFIG:-0}" != 1 ]]; then
                bash "$root_dir/apps/tasktjik/scripts/install.sh"
            fi
            source_dir="$root_dir/apps/notjik/dist"
            target_dir="$HOME/.local/lib/notjik"
            install -d -m 755 "$HOME/.local/bin" "$target_dir" "$HOME/.config/systemd/user"
            cp -R "$source_dir"/qml "$source_dir"/themes "$source_dir"/examples "$target_dir/"
            install -d -m 755 "$target_dir/bin"
            install -m 755 "$source_dir/bin/notjik-core" "$target_dir/bin/.notjik-core.pending"
            mv -f "$target_dir/bin/.notjik-core.pending" "$target_dir/bin/notjik-core"
            install -m 755 "$source_dir/bin/notjik" "$source_dir/bin/notjikctl" "$target_dir/bin/"
            install -m 644 "$source_dir/install-telegram.sh" "$source_dir/README.md" "$target_dir/"
            rm -f "$target_dir"/{notjik,lock_watch,telegram_bridge,telegram_recording,telegram_media_render,comparison,demo,demo_cycle}.py "$target_dir/requirements-telegram.txt"
            rm -rf "$target_dir/__pycache__"
            install -m 755 "$source_dir/bin/notjik" "$source_dir/bin/notjikctl" "$HOME/.local/bin/"
            bash "$root_dir/scripts/install-completions.sh" "$HOME/.local" notjik
            if [[ "${TJIKUP_MANAGED_CONFIG:-0}" != 1 ]]; then
                install -m 644 "$source_dir/systemd/notjik.service" "$HOME/.config/systemd/user/notjik.service"
                install -m 644 "$source_dir/systemd/notjik-telegram.service" "$HOME/.config/systemd/user/notjik-telegram.service"
                activation_dir="${XDG_DATA_HOME:-$HOME/.local/share}/dbus-1/services"
                install -d -m 755 "$activation_dir"
                printf '%s\n' '# Activate the notjik desktop notification service.' '[D-BUS Service]' 'Name=org.freedesktop.Notifications' \
                    "Exec=$HOME/.local/bin/notjik" 'SystemdService=notjik.service' \
                    > "$activation_dir/org.freedesktop.Notifications.service"
                chmod 644 "$activation_dir/org.freedesktop.Notifications.service"
            fi
            ;;
    esac
done
