#!/usr/bin/env bash
set -euo pipefail
app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
engine_dir="$app_dir/../../packages/whatsover-engine"
export RUSTFLAGS="${RUSTFLAGS:-} --cfg getrandom_backend=\"wasm_js\""
features="wasm"
if [[ "${WHATSOVER_QUALIFICATION:-0}" == "1" ]]; then features="wasm,qualification"; fi
cargo build --manifest-path "$engine_dir/Cargo.toml" --locked --no-default-features --features "$features" --target wasm32-unknown-unknown --release
build_dir="${CARGO_TARGET_DIR:-$engine_dir/target}"
wasm-bindgen "$build_dir/wasm32-unknown-unknown/release/whatsover_engine.wasm" --target web --out-dir "$app_dir/dist/engine"
bun "$app_dir/scripts/wrap-bindings.ts"
