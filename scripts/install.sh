#!/usr/bin/env bash
set -euo pipefail
root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
install -d -m 755 "$HOME/.local/bin" "$HOME/.local/lib/mailtjik"
install -m 755 "$root_dir/apps/mailtjik/dist/mailtjik" "$HOME/.local/bin/mailtjik"
install -m 644 "$root_dir"/apps/mailtjik/dist/backend/*.py "$HOME/.local/lib/mailtjik/"
