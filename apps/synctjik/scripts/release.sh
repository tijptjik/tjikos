#!/usr/bin/env bash
set -euo pipefail
app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
target=x86_64-unknown-linux-musl
cargo build --locked --release --target "$target" --manifest-path "$app_dir/Cargo.toml"
release_dir="$app_dir/dist/release"
mkdir -p "$release_dir"
binary="$release_dir/synctjik-$target"
install -m 755 "$app_dir/target/$target/release/synctjik" "$binary"
if readelf -l "$binary" | grep -q INTERP; then
    echo 'Release must be statically linked; dynamic interpreter found' >&2
    exit 1
fi
"$binary" --version
source_revision="$(git -C "$app_dir" rev-parse HEAD)"
compiler="$(rustc --version)"
printf '{"source_revision":"%s","target":"%s","compiler":"%s"}\n' \
    "$source_revision" "$target" "$compiler" > "$release_dir/build-info.json"
(cd "$release_dir" && sha256sum "synctjik-$target" build-info.json > SHA256SUMS)
