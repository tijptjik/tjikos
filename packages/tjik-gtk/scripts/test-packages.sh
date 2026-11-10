#!/usr/bin/env bash
# Run through dev-check on managed desktops. No registry writes are performed.
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$repo_root"
bash packages/tjik-gtk/scripts/native.sh
native_lib="$repo_root/packages/tjik-gtk/target/native/lib"
export PKG_CONFIG_PATH="$native_lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$native_lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export CARGO_TARGET_DIR="$repo_root/packages/tjik-gtk/target/package-review"
review_root="$(mktemp -d /tmp/tjik-package-review.XXXXXX)"
printf 'Extracted package review: %s\n' "$review_root"
# Stage the Git-visible package inputs so Cargo's package normalization cannot
# rewrite working-tree lockfiles or race with another application's build.
python3 - "$review_root/source" <<'PYSTAGE'
from pathlib import Path
import shutil
import subprocess
import sys

destination = Path(sys.argv[1])
paths = subprocess.check_output([
    "git", "ls-files", "-z", "--cached", "--others", "--exclude-standard", "--",
    "packages/tjik-desktop", "packages/tjik-gtk", "packages/tasktjik-gtk",
]).decode().split("\0")
for path in set(paths) - {""}:
    source = Path(path)
    if not source.is_file():
        continue
    target = destination / source
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, target, follow_symlinks=False)
PYSTAGE
for directory in tjik-desktop tjik-gtk tasktjik-gtk; do
    manifest="$review_root/source/packages/$directory/Cargo.toml"
    crate="$(python3 -c 'import sys,tomllib; print(tomllib.load(open(sys.argv[1], "rb"))["package"]["name"])' "$manifest")"
    version="$(python3 -c 'import sys,tomllib; print(tomllib.load(open(sys.argv[1], "rb"))["package"]["version"])' "$manifest")"
    config=()
    if [[ "$directory" == tasktjik-gtk ]]; then
        # Resolve the not-yet-published dependency from its extracted archive.
        # This proves archive isolation, not registry name/version availability.
        config=(--config "patch.crates-io.$gtk_name.path=\"$gtk_archive\"")
    fi
    # Packaging normalizes path dependencies and its temporary lockfile. Cargo
    # cannot do this with --locked when an unpublished dependency is patched.
    cp "$review_root/source/packages/$directory/Cargo.lock" "$review_root/$crate.source.lock"
    cargo package --manifest-path "$manifest" --offline --allow-dirty --no-verify "${config[@]}"
    tar -xzf "$CARGO_TARGET_DIR/package/$crate-$version.crate" -C "$review_root"
    archive="$review_root/$crate-$version"
    python3 - "$manifest" "$archive" <<'PYLICENSE'
from pathlib import Path
import sys
import tomllib

source = Path(sys.argv[1]).parent
archive = Path(sys.argv[2])
with (archive / "Cargo.toml").open("rb") as stream:
    package = tomllib.load(stream)["package"]
    expected = "MPL-2.0 AND BSD-3-Clause" if source.name == "tjik-desktop" else "MPL-2.0"
    assert package["license"] == expected
for name in ("LICENSE", "NOTICE"):
    assert (archive / name).read_bytes() == (source / name).read_bytes(), name
PYLICENSE
    if [[ "$directory" == tjik-gtk ]]; then
        gtk_archive="$archive"
        gtk_name="$crate"
    fi
    python3 - "$review_root/$crate.source.lock" "$archive/Cargo.lock" <<'PYLOCK'
import sys
import tomllib

def packages(path):
    with open(path, "rb") as stream:
        return {(p["name"], p["version"], p.get("checksum"))
                for p in tomllib.load(stream)["package"]}

extra = packages(sys.argv[2]) - packages(sys.argv[1])
assert not extra, f"Archive introduced dependencies absent from source lock: {extra}"
PYLOCK
    # Locked offline resolution prevents a dependency refresh during validation.
    cargo test --manifest-path "$archive/Cargo.toml" --locked --offline "${config[@]}"
    if [[ "$directory" == tjik-desktop ]]; then
        cargo test --manifest-path "$archive/Cargo.toml" --locked --offline --all-features
    fi
done
