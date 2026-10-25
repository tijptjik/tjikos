#!/usr/bin/env bash
set -euo pipefail
root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if (( $# == 0 )); then
    set -- mailtjik tjikup
fi
# Validate all targets before installing any of them.
for app in "$@"; do
    case "$app" in
        mailtjik|tjikup) ;;
        *) printf 'Unknown app: %s (expected mailtjik or tjikup)\n' "$app" >&2; exit 2 ;;
    esac
done
for app in "$@"; do
    case "$app" in
        tjikup)
            bash "$root_dir/apps/tjikup/scripts/install.sh"
            ;;
        mailtjik)
            install -d -m 755 "$HOME/.local/bin" "$HOME/.local/lib/mailtjik"
            install -m 755 "$root_dir/apps/mailtjik/dist/mailtjik" "$HOME/.local/bin/mailtjik"
            install -m 644 "$root_dir"/apps/mailtjik/dist/backend/*.py "$HOME/.local/lib/mailtjik/"
            ;;
    esac
done
