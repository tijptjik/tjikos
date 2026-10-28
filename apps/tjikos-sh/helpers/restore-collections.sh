#!/usr/bin/env bash
# Guided restoration of declared device state; no backup before readiness.
set -euo pipefail
umask 077
dotjik_dir="${1:?dotjik checkout required}"
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}"
mkdir -p "$config_dir/synctjik"
# The token was interactively unlocked earlier. Parse only its assignment.
if [[ -z "${BWS_ACCESS_TOKEN:-}" ]]; then
    while IFS= read -r line || [[ -n "$line" ]]; do
        [[ "$line" != BWS_ACCESS_TOKEN=* ]] || export BWS_ACCESS_TOKEN="${line#BWS_ACCESS_TOKEN=}"
    done < "$HOME/.config/bws/environment"
fi
[[ -n "${BWS_ACCESS_TOKEN:-}" ]] || { echo 'Unlock BWS before archive restoration.' >&2; exit 1; }
# Keep the archive transport separate from media mounts and other rclone remotes.
transport="$config_dir/synctjik/rclone.conf"
[[ ! -L "$transport" ]] || { echo 'Refusing a symlinked transport config.' >&2; exit 1; }
if [[ ! -f "$transport" ]]; then
    temporary="$(mktemp "$config_dir/synctjik/.rclone.XXXXXX")"
    trap 'rm -f -- "${temporary:-}"' EXIT
    chezmoi --source "$dotjik_dir/home" execute-template < "$dotjik_dir/home/dot_config/rclone/private_rclone.conf.tmpl" > "$temporary"
    chmod 600 "$temporary"
    mv -- "$temporary" "$transport"
fi
# Personal archive policy stays in dotjik; never replace an existing local policy.
policy="$config_dir/archive-policy.json"
[[ ! -L "$policy" ]] || { echo 'Refusing a symlinked archive policy.' >&2; exit 1; }
if [[ ! -f "$policy" ]]; then
    install -m 600 "$dotjik_dir/home/dot_config/archive-policy.json" "$policy"
fi
synctjik check
if [[ ! -f "$config_dir/synctjik/device.json" ]]; then
    role="$(jq -er '.role' "$config_dir/tjikup/device.json")"
    synctjik enroll --role "$role" </dev/tty
fi
# Readiness, not the existence of an enrollment file, determines completion.
if ! synctjik ready; then
    synctjik setup </dev/tty
fi
synctjik ready
synctjik schedule
unset BWS_ACCESS_TOKEN
