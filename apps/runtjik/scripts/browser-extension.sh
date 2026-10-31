#!/usr/bin/env bash
set -euo pipefail
export NO_UPDATE_NOTIFIER=1
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
action="${1:-build}"
if [[ $# -gt 0 ]]; then shift; fi
source_dir="$PWD/browser-extension"
output_dir="$PWD/browser-artifacts"
stage_dir="$PWD/dist/browser-addon"
case "$action" in
    dev)
        # A separate development profile; never change the user's daily profile.
        exec bun x --no-install web-ext run --source-dir "$source_dir" \
            --ignore-files README.md install-host.sh PRIVACY.md --firefox "${RUNTJIK_DEV_BROWSER:-zen-browser}" "$@"
        ;;
    lint|build|sign-listed|sign-unlisted) ;;
    *) printf 'Usage: %s {dev|lint|build|sign-listed|sign-unlisted}\n' "$0" >&2; exit 2 ;;
esac
rm -rf -- "$stage_dir"
mkdir -p "$stage_dir" "$output_dir"
# Explicit allowlist: no native installer, signing credentials or profile data.
cp "$source_dir/manifest.json" "$source_dir/background.js" "$source_dir/icon.png" "$stage_dir/"
bun x --no-install web-ext lint --source-dir "$stage_dir" --warnings-as-errors
if [[ "$action" == lint ]]; then exit 0; fi
if [[ "$action" == build ]]; then
    exec bun x --no-install web-ext build --source-dir "$stage_dir" \
        --artifacts-dir "$output_dir" --overwrite-dest
fi
: "${WEB_EXT_API_KEY:?Set WEB_EXT_API_KEY outside the repository}"
: "${WEB_EXT_API_SECRET:?Set WEB_EXT_API_SECRET outside the repository}"
channel="${action#sign-}"
args=(sign --source-dir "$stage_dir" --artifacts-dir "$output_dir" --channel "$channel")
if [[ "$channel" == listed ]]; then
    args+=(--amo-metadata "$PWD/browser-release/amo-metadata.json")
fi
exec bun x --no-install web-ext "${args[@]}" "$@"
