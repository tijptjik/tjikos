#!/usr/bin/env bash
set -euo pipefail
root="${RUNTJIK_HOME:-$HOME/.local/lib/runtjik}"
exec "$root/runtjik-core" register-browser-host "$@"
