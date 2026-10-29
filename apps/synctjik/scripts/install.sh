#!/usr/bin/env bash
set -euo pipefail
app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
prefix="${1:-$HOME/.local}"
bash "$app_dir/scripts/fetch-release.sh"
supports_backups() {
    "$1" capabilities 2>/dev/null | python3 -c 'import json,sys; c=json.load(sys.stdin); sys.exit(not (c.get("cloudflare_backup") and c.get("private_assets")))' 2>/dev/null
}
if ! supports_backups "$app_dir/dist/synctjik"; then
    if [[ -x "$prefix/bin/synctjik" ]] && supports_backups "$prefix/bin/synctjik"; then
        echo 'Synctjik: awaiting compatible release; installed runner retained.'
        exit 0
    fi
    echo 'Pinned Synctjik release lacks backup integrations; publish a compatible release before installing.' >&2
    exit 1
fi
install -d -m 755 "$prefix/bin"
temporary="$(mktemp "$prefix/bin/.synctjik.XXXXXX")"
trap 'rm -f -- "$temporary"' EXIT
install -m 755 "$app_dir/dist/synctjik" "$temporary"
mv -f -- "$temporary" "$prefix/bin/synctjik"
# Dotjik owns compatibility commands during managed installs. Rewriting them
# here would invalidate the file deployer's last-applied baseline.
if [[ "${TJIKUP_MANAGED_CONFIG:-0}" == 1 ]]; then
    exit 0
fi
# Compatibility names delegate to the native engine; existing workers are untouched.
for name in device-archives chezmoi-externals-backup; do
    temporary="$(mktemp "$prefix/bin/.synctjik.XXXXXX")"
    if [[ "$name" == device-archives ]]; then
        printf '%s\n' '#!/usr/bin/env bash' 'exec "$(dirname -- "$0")/synctjik" "$@"' > "$temporary"
    else
        printf '%s\n' '#!/usr/bin/env bash' 'exec "$(dirname -- "$0")/synctjik" backup "$@"' > "$temporary"
    fi
    chmod 755 "$temporary"
    mv -f -- "$temporary" "$prefix/bin/$name"
done
