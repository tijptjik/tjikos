#!/usr/bin/env bash
set -euo pipefail
root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
prefix="${1:?Usage: install-completions.sh PREFIX [APP ...]}"
shift
if (( $# == 0 )); then
    set -- bartjik greetjik mailtjik notjik runtjik synctjik tjikup
fi
files=(__tjikos)
for app in "$@"; do
    case "$app" in
        bartjik) files+=(bartjik bartjikctl) ;;
        notjik) files+=(notjik notjikctl) ;;
        tjikup) files+=(tjikup dotjik systjik tjix tjixx fleet-ssh fitjikup litjikup sitjikup synctjik) ;;
        synctjik) files+=(synctjik device-archives) ;;
        greetjik|mailtjik|runtjik) files+=("$app") ;;
        *) printf 'Unknown completion app: %s\n' "$app" >&2; exit 2 ;;
    esac
done
# Validate the full selection before writing. This installer never starts apps.
for name in "${files[@]}"; do
    test -f "$root_dir/completions/fish/$name.fish"
done
destination="$prefix/share/fish/vendor_completions.d"
install -d -m 755 "$destination"
temporary=''
trap '[[ -z "$temporary" ]] || rm -f -- "$temporary"' EXIT
for name in "${files[@]}"; do
    temporary="$(mktemp "$destination/.tjikos.XXXXXXXX")"
    install -m 644 "$root_dir/completions/fish/$name.fish" "$temporary"
    mv -f -- "$temporary" "$destination/$name.fish"
    temporary=''
done
