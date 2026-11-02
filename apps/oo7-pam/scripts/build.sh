#!/usr/bin/env bash
set -euo pipefail
app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
before="$(bash "$app_dir/scripts/source-digest.sh")"
cargo build --locked --release --manifest-path "$app_dir/Cargo.toml"
[[ "$before" == "$(bash "$app_dir/scripts/source-digest.sh")" ]] || {
    echo 'PAM source changed during compilation; rebuild before installing.' >&2
    exit 1
}
mkdir -p "$app_dir/dist"
if ! cmp -s "$app_dir/target/release/libpam_oo7_tjik.so" "$app_dir/dist/pam_oo7_tjik.so"; then
    install -m 755 "$app_dir/target/release/libpam_oo7_tjik.so" "$app_dir/dist/pam_oo7_tjik.so"
fi
printf '%s\n' "$before" > "$app_dir/dist/source.sha256"
