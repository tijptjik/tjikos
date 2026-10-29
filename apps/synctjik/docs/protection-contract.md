# Native protection engines

Compatibility inventory recorded before replacement, 2026-10-29. The frozen
sources in `tests/legacy/protection` are test oracles only, never installed or
invoked by Synctjik. Personal paths, selection, identities and schedules belong
to Dotjik JSON policy. Existing encrypted repositories need no conversion.

| Engine | Public entry | Durable contract |
| --- | --- | --- |
| Drive | `drive backup`, `drive restore` | Read-only rclone export mount; docx,xlsx,pptx,svg; one snapshot per selected tag; host/tag retention 1 daily/weekly/monthly; Restic retry-lock 15m; operation.lock nonblocking; last-success.json completed/repository/sources; last-error.json time/error. Empty sources fail. Check precedes retention. |
| Applications | `app-backup backup` | Shared backup flock; disk UUID and data/pool mount checks before staging; rsync exclusions and vanished-live-file tolerance (24 only); SQLite online backups, quick_check, Plex custom SQLite and historical .db.bak distinction; narrowly configured backup-only index repair with original evidence; locked all-database MariaDB dump; existing backup-manifest.json and absolute source paths; --force content reads; 7 daily/4 weekly host,tag retention; last-success.json completedAt/repository only after check. Applications stay online. |
| Independent copy | `app-pull copy`, `app-pull health` | copy.lock blocking; verified SSH endpoints and hostname; independent password/chunker params; all retained host/tag snapshots, not latest-only; 2048 KiB/s cap; 3 attempts with 10/20 second backoff; 36h source freshness (health 24h); latest Sunday 06:00 HKT receipt boundary; 7 daily/4 weekly; full read-data check; copiedAt/sourceSnapshot/sourceTime/dataCheck receipt. |
| Recovery | `app-restore stage/install/import-mariadb` | Exact 64-character snapshot ID; new staging target; Restic --verify then manifest/SQLite/dump checks; allowlisted empty destinations all prechecked before copying; stopped writers; identity and mount checks; same backup lock nonblocking; MariaDB target mount and empty app tables checked; started receipt before import, complete after; no automatic retry of a partial import. |
| SnapRAID | `snapraid guard/status/preflight/sync/scrub/resume` | Maintenance flock then backup flock; UUID/rw mounts, exact ordered layout and no other snapraid process; complete removed summary, <= configured threshold; no force flags; scrub 25% older than 7 days; failed scrub blocks sync; durable paused.json written before pause and cleared only after all owned writers resume; last-{mode}-success.json completed_at/mode and {mode}-failure.json failed_at/error. Status busy exits 75 without parity work. |
| Notifications | `snapraid-notify failure/recovery/health` | notify.lock; unit allowlist; HTML escaped messages; 24h unchanged-incident reminders; failed delivery leaves pending incident; recovery only after an active incident; receipt freshness and monotonic running duration; queued failures reconcile recovered units; token never in errors. No parity operations. |

Existing unit names/timers, lock paths, credential paths and receipt schemas stay
stable. System/user scope, conditions, OnFailure/OnSuccess, ExecStopPost recovery,
timeouts and service sandboxing stay in Dotjik. Schedule migration must defer
while any relevant worker or lock is busy; no service restart, timer enable, or
broad apply is part of validation. Release publication/pins remain deferred.

Safety improvements must be additive: fsync plus atomic receipt publication,
non-symlink paths, cancellation cleanup, rejection of future-dated copy evidence,
and verified copy membership before retention. A receipt does not substitute for
an actual restored-content/hash test. Synthetic failure and restore evidence,
installed routing inspection, and live private-data verification are separate
acceptance gates.
