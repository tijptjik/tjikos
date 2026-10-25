#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
requirements="$script_dir/requirements-telegram.txt"
if [[ ! -f "$requirements" ]]; then
    requirements="$script_dir/../requirements-telegram.txt"
fi
runtime="$HOME/.local/share/notjik/telegram-venv"
command -v uv >/dev/null || { printf 'Install uv before setting up Telegram support.\n' >&2; exit 1; }
if [[ ! -x "$runtime/bin/python" ]]; then
    uv venv --python /usr/bin/python3 "$runtime"
fi
uv pip install --python "$runtime/bin/python" --requirement "$requirements"
printf 'Telegram runtime ready. Run: notjikctl telegram login\n'
