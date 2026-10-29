#!/usr/bin/env bash
set -euo pipefail
app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "${1:-}" == --from-build ]]; then
    shift
    source_binary="$app_dir/target/release/synctjik"
else
    bash "$app_dir/scripts/fetch-release.sh"
    source_binary="$app_dir/dist/synctjik"
fi
prefix="${1:-$HOME/.local}"
supports_backups() {
    local capabilities
    capabilities="$("$1" capabilities 2>/dev/null)" || return 1
    grep -Eq '"native_backends"[[:space:]]*:[[:space:]]*1([,}])' <<< "$capabilities" &&
        grep -Eq '"protection_backends"[[:space:]]*:[[:space:]]*1([,}])' <<< "$capabilities"
}
if ! supports_backups "$source_binary"; then
    if [[ "$source_binary" == "$app_dir/target/release/synctjik" ]]; then
        echo 'Local Synctjik build lacks native protection engines; build the current source before installing.' >&2
        exit 1
    fi
    if [[ -x "$prefix/bin/synctjik" ]] && supports_backups "$prefix/bin/synctjik"; then
        echo 'Synctjik: awaiting compatible release; installed runner retained.'
        exit 0
    fi
    echo 'Pinned Synctjik release lacks native protection engines; publish a compatible release before installing.' >&2
    exit 1
fi
install -d -m 755 "$prefix/bin"
temporary="$(mktemp "$prefix/bin/.synctjik.XXXXXX")"
trap 'rm -f -- "$temporary"' EXIT
install -m 755 "$source_binary" "$temporary"
mv -f -- "$temporary" "$prefix/bin/synctjik"
bash "$app_dir/../../scripts/install-completions.sh" "$prefix" synctjik
# Dotjik owns compatibility commands during managed installs. Rewriting them
# here would invalidate the file deployer's last-applied baseline.
if [[ "${TJIKUP_MANAGED_CONFIG:-0}" == 1 ]]; then
    exit 0
fi
# Keep the archive CLI alias; retired deployment commands are not installed.
temporary="$(mktemp "$prefix/bin/.synctjik.XXXXXX")"
printf '%s\n' '#!/usr/bin/env bash' 'exec "$(dirname -- "$0")/synctjik" "$@"' > "$temporary"
chmod 755 "$temporary"
mv -f -- "$temporary" "$prefix/bin/device-archives"
