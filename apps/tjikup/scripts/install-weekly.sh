#!/usr/bin/env bash
# Install a separately versioned scheduled runtime from an already-built artifact.
# Ordinary application installation must not downgrade this runtime.
set -euo pipefail
app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${1:-$app_dir/dist/tjikup}"
"$binary" scheduled --help >/dev/null
sudo -n install -d -m 755 /usr/local/libexec
staged="$(sudo -n mktemp /usr/local/libexec/.tjikup-weekly.XXXXXX)"
trap 'sudo -n rm -f -- "$staged"' EXIT
sudo -n install -m 755 "$binary" "$staged"
sudo -n mv -f "$staged" /usr/local/libexec/tjikup-weekly
