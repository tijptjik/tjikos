#!/usr/bin/env bash
set -euo pipefail
app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
prefix="${1:-$HOME/.local}"
bash "$app_dir/scripts/fetch-release.sh"
install -d -m 755 "$prefix/bin"
temporary="$(mktemp "$prefix/bin/.synctjik.XXXXXX")"
trap 'rm -f -- "$temporary"' EXIT
install -m 755 "$app_dir/dist/synctjik" "$temporary"
mv -f -- "$temporary" "$prefix/bin/synctjik"
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
