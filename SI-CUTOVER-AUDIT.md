# Si local-policy audit

Initially inspected read-only on 2026-10-29 through the existing pinned SSH
identity. The user subsequently authorized reconciliation and a Tjikup run.

## Cutover execution

- Preserved both old checkouts as verified private worktree archives and Git
  bundles under
  `/home/m/.local/state/tjikup/si-native-cutover-20261029T151734Z/`.
  Local Telegram edits are also retained in recovery commits `94260ca` (home)
  and `8c46d83` (system).
- Merged the home changes into their equivalent native paths. The server policy
  checkout is at `2375e2d`; TjikOS is at `52adbe5`. No repository was pushed.
- Installed native Tjikup for the user and at the root-owned system runtime.
  Existing native Synctjik binaries already supported protection backends and
  were retained. Enrolled Si explicitly as a server and installed Fish completions.
- Applied the reviewed system policy through the Synctjik guard: 42 changes,
  with private rollback receipt
  `/var/lib/tjikup/files/receipt-HfxMj0/receipt.json`. System checks now converge.
  Backup, SnapRAID and Transmission units call native commands. Nginx validation
  and storage checks pass; timer enablement and container IDs are unchanged.
- Preserved Si's existing Herdr session. Shared desktop captures now select only
  Fi/Li. Corrected the legacy age identity from 0644 to 0600; native Tjikup created
  its private `age.txt` copy without changing key material.

The home run is **pending**, not successful yet. It reached the normal guard and
returned 75 because `drive-restic-backup.service` still holds its lock. The
existing backup and `synctjik-drive-restore-validation-20261029.service` were left
running. A one-shot `tjikup-si-native-cutover.service` is queued after both jobs.
It checks the approved Git revisions and live-file snapshots, records reviewed
migration baselines through the native file engine, corrects exactly two
root-owned Beets helper paths with recovery receipts, then runs:

```sh
tjikup --skip-repo-sync --skip-package-updates --show-skips
```

It retains the ordinary protection guards and stops if reviewed inputs changed.
Logs, exit status and completion timestamp will be written to `queued-run.log`,
`queued-run.exit` and `queued-run.finished` in the private cutover directory.
Check the unit's queued job with `systemctl --user list-jobs`; an inactive state
alone does not mean a queued one-shot has completed.

## What the local edits contain

The home checkout at `a607309` has one modified Compose template (123 added
lines) and an untracked Telegram Archive guide and helper directory. The
separate system checkout has one modified nginx site (26 added lines).
All additions implement the Telegram Archive deployment:

- `telegram-backup`: pinned 9.5.0 collector/listener, archived-chat adapter,
  private Telegram session, soft deletion, media downloads and voice transcription.
- `telegram-viewer`: pinned 9.5.0 authenticated viewer on loopback port 8096,
  with the prebuilt Tailwind stylesheet and separate viewer credentials.
- `akou`: pinned 0.5.4 CPU transcription service, private Docker network,
  persistent model/job storage and resource limits.
- nginx: `tga.type.hk`, the static stylesheet substitution, streaming proxy and
  private/no-store response policy.

## Coverage in native policy

| Local addition | Comparison with current native source |
| --- | --- |
| Telegram Compose services and network | Equivalent after Go-to-MiniJinja syntax conversion and resolving `ports.telegramArchive` to 8096; only extra whitespace otherwise |
| `akou-config.json` | Byte-for-byte identical; managed on Si only |
| `all_chats.py` | Byte-for-byte identical; managed on Si only |
| `build_css.py` | Byte-for-byte identical; managed on Si only |
| `tailwind-9.5.0.css` | Byte-for-byte identical; managed on Si only |
| Telegram nginx block | Byte-for-byte identical; the rest of the site differs only in blank lines |
| Telegram Archive guide | Native guide contains expanded instructions and verification; its incorrect live checkout paths were corrected during this audit |

The running collector, viewer and akou containers were healthy. This establishes
service health, not archive completeness or recoverability. The local changes
must still be preserved before reconciling Si's old checkouts, even though their
behavior is represented in the current native source.

## Transmission is a different deployment on Si

Fi/Li run the GTK desktop client using `~/.config/transmission/settings.json`.
Si runs the LinuxServer daemon container using appData mounted at `/config`,
shared storage at `/data/meta/downloads`, and its existing RPC endpoint. Its
container was healthy and its cleanup timer was active at inspection time.

Desktop settings are explicitly limited to Fi/Li. The server startup hook is
server-only; desktop extraction is client-only. The server hook disables the
old extraction script in favor of Unpackerr, sets shared-media umask 002 and
enforces the daily 09:00–17:00 HKT bandwidth schedule. Native Si maintenance
retains host-bound cleanup policy and its protection gates. The GTK favicon
repair does not change the daemon image, RPC settings, downloads or cleanup jobs.

## Remaining cutover gates

1. Let the existing backup and restore validation finish; inspect their evidence
   separately from the Tjikup cutover result.
2. Confirm the queued home run exits successfully and its native home/system
   checks converge. Investigate any new conflict; do not force an overwrite.
3. Retire remaining legacy path aliases and entrypoints after home acceptance.
   Keep the private recovery material until it is no longer needed.

No archive worker or container was stopped or replaced during this cutover.
