#!/usr/bin/env bash
# Install only native TDLib/rlottie libraries. Wheel containers are unpacked as
# archives; no Python interpreter, extension module, or virtualenv is installed.
set -euo pipefail
native="${NOTJIK_NATIVE_DIR:-$HOME/.local/share/notjik/native}"
mkdir -p "$native"
chmod 700 "$native"
install_native() {
    local name="$1" package="$2" version="$3" pattern="$4" source=""
    if [[ -f "$native/lib$name.so" ]]; then return; fi
    # One-time extraction from an existing installation, never a runtime fallback.
    shopt -s nullglob
    local candidates=("$HOME"/.local/share/notjik/telegram-venv/lib/python*/site-packages/$pattern)
    shopt -u nullglob
    for candidate in "${candidates[@]}"; do
        if [[ -f "$candidate" ]]; then source="$candidate"; break; fi
    done
    if [[ -n "$source" ]]; then
        install -m 755 "$source" "$native/.lib$name.so.pending"
        mv -f "$native/.lib$name.so.pending" "$native/lib$name.so"
        return
    fi
    for command in curl jq unzip sha256sum rg; do command -v "$command" >/dev/null || { echo "Install $command for native Telegram setup" >&2; return 1; }; done
    local arch metadata wheel url digest temporary
    arch="$(uname -m)"
    case "$arch" in x86_64|aarch64) ;; *) echo "Provide native TDLib and rlottie libraries for $arch" >&2; return 1;; esac
    temporary="$(mktemp -d)"
    metadata="$(curl --fail --silent --show-error "https://pypi.org/pypi/$package/$version/json")"
    wheel="$(jq -cer --arg arch "$arch" '[.urls[] | select(.filename | contains("manylinux")) | select(.filename | contains($arch))] | first // error("No native wheel for this platform")' <<<"$metadata")"
    url="$(jq -r '.url' <<<"$wheel")"
    digest="$(jq -r '.digests.sha256' <<<"$wheel")"
    curl --fail --silent --show-error --location "$url" -o "$temporary/native.whl"
    printf '%s  %s\n' "$digest" "$temporary/native.whl" | sha256sum --check --status
    unzip -q "$temporary/native.whl" -d "$temporary/unpacked"
    source="$(rg --files --hidden "$temporary/unpacked" | rg "/lib$name([-.].*)?\\.so(\\..*)?$" | head -n 1)"
    [[ -n "$source" ]] || { echo "Native $name library missing from archive" >&2; rm -rf "$temporary"; return 1; }
    install -m 755 "$source" "$native/.lib$name.so.pending"
    mv -f "$native/.lib$name.so.pending" "$native/lib$name.so"
    rm -rf "$temporary"
}
install_native tdjson tdjson 1.8.69 'tdjson.libs/libtdjson-*.so*'
install_native rlottie rlottie-python 1.3.8 'rlottie_python/librlottie.so.0.2'
printf 'Native Telegram libraries ready. Run: notjikctl telegram doctor\n'
