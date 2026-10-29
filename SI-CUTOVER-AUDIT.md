# Si local-policy audit

Inspected 2026-10-29 through the existing pinned SSH identity. No files, containers,
timers, credentials or archive workers on Si were changed.

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

1. Preserve both dirty checkouts and reconcile them with the native layouts.
2. Install and verify capable user/system Tjikup and Synctjik runners; Si still
   has the Python Tjikup runner and lacks the native Synctjik entrypoints.
3. Review a Si-specific home/system plan against its live mounts, container
   policy, ownership, services, protection locks and current receipts.
4. Apply only through the native protection guards, without interrupting pinned
   workers; verify fresh backup/restore evidence separately from service health.

This audit resolves the known local-diff question. It does not substitute for
the live server plan and protection checks, and it does not authorize a reset,
clean, whole-stack recreation, or deletion of the old recovery material.
