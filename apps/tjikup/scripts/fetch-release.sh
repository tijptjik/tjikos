#!/usr/bin/env bash
# Fetch the pinned executable; compilation belongs to the release build.
set -euo pipefail
app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$app_dir/../tjikos-sh/release.json"
[[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || {
    echo 'This release supports Linux x86_64.' >&2; exit 1;
}
url="$(jq -er '.tjikup_binary.url' "$manifest")"
checksum="$(jq -er '.tjikup_binary.sha256' "$manifest")"
[[ "$url" =~ ^https://github.com/tijptjik/tjikos/releases/download/tjikup-v[A-Za-z0-9._-]+/tjikup-x86_64-unknown-linux-musl$ && "$checksum" =~ ^[a-f0-9]{64}$ ]] || {
    echo 'No valid pinned tjikup binary in release.json' >&2; exit 1;
}
mkdir -p "$app_dir/dist"
output="$app_dir/dist/tjikup"
if [[ -f "$output" ]] && [[ "$(sha256sum "$output" | cut -d ' ' -f 1)" == "$checksum" ]]; then
    exit 0
fi
temporary="$(mktemp "$app_dir/dist/.tjikup.XXXXXX")"
trap 'rm -f -- "$temporary"' EXIT
curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL --retry 3 --connect-timeout 15 --max-time 180 "$url" -o "$temporary"
[[ "$(sha256sum "$temporary" | cut -d ' ' -f 1)" == "$checksum" ]] || {
    echo 'Tjikup download checksum mismatch; existing executable preserved.' >&2; exit 1;
}
chmod 755 "$temporary"
"$temporary" --version >/dev/null
mv -f -- "$temporary" "$output"
