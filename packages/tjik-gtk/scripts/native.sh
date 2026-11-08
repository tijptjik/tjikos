#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
revision=1c963c51514581c41b9bdae08cdf69171265cdda
source_dir="$root/target/layer-shell-source"
prefix="$root/target/native"
if [[ ! -d "$source_dir/.git" ]]; then
    git clone --no-checkout https://github.com/wmww/gtk4-layer-shell.git "$source_dir"
    git -C "$source_dir" checkout --detach "$revision"
fi
[[ "$(git -C "$source_dir" rev-parse HEAD)" == "$revision" ]] || { echo 'Unexpected gtk4-layer-shell source revision' >&2; exit 1; }
if [[ ! -f "$prefix/lib/pkgconfig/gtk4-layer-shell-0.pc" ]]; then
    meson setup "$root/target/layer-shell-build" "$source_dir" --prefix="$prefix" --libdir=lib --buildtype=release -Dintrospection=false -Dvapi=false -Dexamples=false -Dtests=false -Dsmoke-tests=false
    meson compile -C "$root/target/layer-shell-build"
    meson install -C "$root/target/layer-shell-build"
fi
