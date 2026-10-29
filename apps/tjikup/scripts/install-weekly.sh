#!/usr/bin/env bash
# Install a separately versioned scheduled runtime from an already-built artifact.
# Ordinary application installation must not downgrade this runtime.
set -euo pipefail
app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${1:-$app_dir/dist/tjikup}"
"$binary" capabilities | jq -e '.scheduled_telegram >= 1 and .scheduled_telegram_bws >= 1' >/dev/null || {
    echo 'Tjikup release lacks BWS-backed scheduled reporting; existing weekly runtime preserved.' >&2
    exit 1
}
sudo -n install -d -m 755 /usr/local/libexec
if sudo -n test -f /usr/local/libexec/tjikup-weekly &&
    sudo -n cmp -s "$binary" /usr/local/libexec/tjikup-weekly &&
    [[ "$(stat -c '%u:%a' /usr/local/libexec/tjikup-weekly)" == '0:755' ]]; then
    exit 0
fi
staged="$(sudo -n mktemp /usr/local/libexec/.tjikup-weekly.XXXXXX)"
trap 'sudo -n rm -f -- "$staged"' EXIT
sudo -n install -m 755 "$binary" "$staged"
sudo -n mv -f "$staged" /usr/local/libexec/tjikup-weekly
