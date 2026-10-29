#!/usr/bin/env bash
# Guided restoration of declared device state; no backup before readiness.
set -euo pipefail
umask 077
dotjik_dir="${1:?dotjik checkout required}"
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}"
# Tjikup renders private transport and policy only when absent. Synctjik owns
# enrollment, restoration, readiness and scheduling.
tjikup workflow --repo "$dotjik_dir" bootstrap-archives
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
