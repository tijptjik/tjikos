#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
account="${1:-default}"
data_dir="${2:-${GMAILTJIK_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/gmailtjik}}"
[[ "$account" =~ ^[a-zA-Z0-9_-]+$ ]] || { echo 'Use letters, numbers, underscores or hyphens for the account instance.' >&2; exit 1; }
[[ "$data_dir" = /* ]] || { echo 'Account data directory must be absolute.' >&2; exit 1; }
test -x "$root/dist/gmailtjik"
test -f "$data_dir/account.json" || { echo 'Connect this account before installing its sync service.' >&2; exit 1; }
install -d -m700 "$HOME/.local/lib/gmailtjik" "$HOME/.config/gmailtjik/accounts"
install -d "$HOME/.config/systemd/user"
install -m755 "$root/dist/gmailtjik" "$HOME/.local/lib/gmailtjik/.gmailtjik.pending"
mv -f "$HOME/.local/lib/gmailtjik/.gmailtjik.pending" "$HOME/.local/lib/gmailtjik/gmailtjik"
python3 - "$data_dir" "$HOME/.config/gmailtjik/accounts/$account.env" <<'PY'
import os,sys
value=sys.argv[1]
if '\n' in value or '\r' in value:raise SystemExit('Invalid account path')
value=value.replace('\\','\\\\').replace('"','\\"')
fd=os.open(sys.argv[2],os.O_WRONLY|os.O_CREAT|os.O_TRUNC,0o600)
with os.fdopen(fd,'w') as stream:stream.write('GMAILTJIK_DATA_DIR="'+value+'"\n')
PY
install -m644 "$root/systemd/gmailtjik-sync@.service" "$HOME/.config/systemd/user/gmailtjik-sync@.service"
systemctl --user daemon-reload
systemctl --user enable "gmailtjik-sync@$account.service"
systemctl --user restart "gmailtjik-sync@$account.service"
echo "Installed and started gmailtjik-sync@$account.service"
