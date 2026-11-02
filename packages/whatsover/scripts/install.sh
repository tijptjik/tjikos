#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
prefix="${WHATSOVER_INSTALL_PREFIX:-$HOME/.local}"
bun_path="$(command -v bun)"
# Deliberately a checkout-linked command; dependencies stay in this workspace.
install -d -m 755 "$prefix/bin"
pending="$(mktemp "$prefix/bin/.whatsover.XXXXXXXX")"
trap 'rm -f -- "$pending"' EXIT
printf '#!/usr/bin/env bash\nexec %q %q "$@"\n' "$bun_path" "$root/src/cli.ts" > "$pending"
chmod 755 "$pending"
mv -f -- "$pending" "$prefix/bin/whatsover"
printf 'Installed %s/bin/whatsover (linked to this checkout). No service started.\n' "$prefix"
