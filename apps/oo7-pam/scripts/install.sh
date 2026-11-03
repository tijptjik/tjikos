#!/usr/bin/env bash
# Install separately from Fedora's pam_oo7.so; never modify the vendor module.
set -euo pipefail
app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
module="$app_dir/dist/pam_oo7_tjik.so"
target=/usr/local/lib/security/pam_oo7_tjik.so
test -s "$module"
test -s "$app_dir/dist/source.sha256"
[[ "$(cat "$app_dir/dist/source.sha256")" == "$(bash "$app_dir/scripts/source-digest.sh")" ]] || {
    echo 'Rebuild oo7-pam before installing: source differs from built artifact.' >&2
    exit 1
}
if [[ "${1:-}" == --check ]]; then
    # A missing installation is drift (1), not cmp's I/O error (2). Preserve
    # errors for existing unreadable targets and dangling symlinks.
    if [[ ! -e "$target" && ! -L "$target" ]]; then
        echo 'oo7-pam is not installed; run the Hyprlock keyring installation.' >&2
        exit 1
    fi
    cmp -s "$module" "$target" && [[ "$(stat -c '%u:%g:%a' "$target")" == 0:0:755 ]]
    exit $?
fi
[[ $# == 0 ]] || { echo 'Usage: install.sh [--check]' >&2; exit 2; }
sudo -n install -d -m 755 -o root -g root /usr/local/lib/security
staged="$(sudo -n mktemp /usr/local/lib/security/.pam_oo7_tjik.XXXXXX)"
trap 'sudo -n rm -f -- "$staged"' EXIT
sudo -n install -m 755 -o root -g root "$module" "$staged"
sudo -n mv -Tf -- "$staged" "$target"
if command -v restorecon >/dev/null 2>&1; then
    sudo -n restorecon "$target"
fi
sudo -n install -D -m 644 -o root -g root "$app_dir/LICENSE" /usr/local/share/licenses/tjikos-oo7-pam/LICENSE
