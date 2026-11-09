#!/usr/bin/env bash
set -euo pipefail
# Build a pinned Hyprvoice release with the explicit local Codex provider.
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cache_root=${XDG_CACHE_HOME:-$HOME/.cache}/hyprvoice-codex-build
mkdir -p "$cache_root"
build_dir=$(mktemp -d "$cache_root/source.XXXXXX")
trap 'rm -rf -- "$build_dir"' EXIT
revision=2b1c94824585f37df260b522e2c343fc1d58ef93
git clone --quiet --depth 1 --branch v1.0.2 https://github.com/leonardotrapani/hyprvoice.git "$build_dir/source"
cd "$build_dir/source"
[[ $(git rev-parse HEAD) == "$revision" ]]
git apply --check "$script_dir/subscription-provider.patch"
git apply "$script_dir/subscription-provider.patch"
git apply --check "$script_dir/empty-text.patch"
git apply "$script_dir/empty-text.patch"
mkdir -p cmd/codex-smoke "$cache_root/tmp"
cp "$script_dir/smoke.go" cmd/codex-smoke/main.go
cp "$script_dir/empty_text_test.go" internal/pipeline/empty_text_test.go
export GOTMPDIR="$cache_root/tmp"
# Skip upstream microphone integration tests; keep capture argument regressions.
CI=true go test ./internal/config ./internal/provider ./internal/llm ./internal/transcriber ./internal/recording
# The upstream notify suite invokes the desktop; this focused test uses a stub.
go test ./internal/notify -run '^TestErrorsOnlyDesktopNotifications$'
go test ./internal/pipeline -run '^TestPipeline_WithMocks'
output=${1:-${XDG_DATA_HOME:-$HOME/.local/share}/hyprvoice-codex}
mkdir -p "$output"
go build -o "$output/hyprvoice.next" ./cmd/hyprvoice
go build -o "$output/codex-smoke.next" ./cmd/codex-smoke
mv "$output/hyprvoice.next" "$output/hyprvoice"
mv "$output/codex-smoke.next" "$output/codex-smoke"
