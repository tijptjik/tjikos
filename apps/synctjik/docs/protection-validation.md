# Protection migration evidence — 2026-10-29

Native implementation and synthetic recovery are complete. Live acceptance is
**not complete**: deployment remains outstanding, and the inspected local routing
still uses the previous Python helper. Release publication and pin changes remain
deferred. No backup-engine deployment, private backup/restore, real parity
operation, notification delivery, or backup schedule activation was performed by
this work. The separate Tailscale trial below did not exercise a backup engine.

## Implementation

- Native Rust commands: `drive`, `app-backup`, `app-pull`, `app-restore`,
  `snapraid`, `snapraid-notify`, and `protection-migration`.
- Dotjik JSON retains source/exclusion selection, export formats, retention,
  disk identities, SSH selection, notification configuration paths, and recovery
  allowlists. Credentials and receipt/lock locations are unchanged.
- Production source units and compatibility aliases delegate to Synctjik;
  obsolete Python engines are removed from both source namespaces. Frozen
  compatibility oracles remain only under test directories.
- `protection-migration` acquires the old locks nonblocking, checks worker state,
  supervises the reviewed apply, and reloads the declared manager before releasing
  locks. It never stops/restarts jobs or enables/disables timers.
- The only Tjikup integration owned by this work is `mod protection` and the
  guarded-apply call in `src/workflow_files.rs`, the new
  `src/workflow_files/protection.rs`, and its focused integration test. Other
  modifications in the shared file belong to the concurrent workstream.
- Restore staging supports replacement hosts. Live installation still verifies
  expected mounts, requires stopped writers, and prechecks empty allowlisted
  destinations. Partial MariaDB imports retain a started marker blocking retries.

## Passing checks

| Check | Evidence |
| --- | --- |
| `bun install` | No dependency changes. |
| `bun run build` | All nine packages built. Synctjik rebuilt after the final restore change. |
| Synctjik `cargo fmt --check`, `cargo check --locked` | Passed. |
| Synctjik `cargo test --locked` | 23 passed; one child-fixture test is deliberately ignored at discovery and launched by its parent test. Includes archive compatibility and native backend tests. |
| `test_protection.py` | All 28 tests passed together in the final pre-commit run, including replacement-host staging and real Drive/application/independent-copy restores. |
| Existing native backend tests | 7 passed. |
| Existing Python compatibility oracles | Cloudflare 15 and private storage 12 passed. |
| Installer checks | 5 passed, including refusal to replace a protection-capable binary with an older native-only release. |
| Tjikup protection cutover/workflow tests | 8 passed; busy apply writes no files, idle apply delegates, active oneshot refresh is refused. |
| Dotjik focused tests | 38 passed: routing/policy 4, SnapRAID 9, historical database roles 1, bootstrap/restore 9, native integration 7, recipes 3, home rendering 5. |
| Dotjik full home/system validation | Home: 318 tests, 9 skipped. System: 42 tests, 1 skipped. Fi/Li/Si task rendering and syntax checks passed. |
| Dotjik policy `check` commands | All five policies validated without credentials or dependency execution. |
| Source invocation scan / shell syntax / diff whitespace | No Python invocation in migrated production routes; passed. |

The protection cases cover upload/backup failure, empty export, unavailable mount,
stale/future/incomplete copy, retry exhaustion, retention and check failure,
receipt publication failure, concurrent locks, active workers, cancelled Drive
and parity workers, retained pause ownership, truncated/excessive deletion diffs,
scrub failure blocking sync, notification delivery retry/dedup/recovery, corrupt
health evidence, SQLite WAL snapshots, invalid/partial restores, populated targets,
and non-retriable partial MariaDB import.

Real Restic rehearsals used only generated data and temporary owner-only
credentials/repositories. Drive document hashes matched after restoration.
Application file/library hashes and SQLite contents matched. An independently
encrypted copy restored successfully after its source repository was renamed
out of reach. A full `check --read-data` passed. These prove synthetic encrypted
storage/restore behavior; they do not prove current private backups, real Google
export conversion, production MariaDB import, or parity integrity.

## Validation limits and installed routing

The final pre-commit `bun run check` passed all nine packages (eight cached).
The earlier formatting failure in concurrently edited Tjikup files is resolved;
those files were not reformatted by this work.

The sandbox denied localhost sockets used by fake HTTP servers and Restic's
restored-directory ownership operation (`lchown ... invalid argument`). The
affected synthetic tests passed when rerun with the required access, using the
same temporary fixtures. No live source or credential was substituted.

Read-only local inspection found:

- Installed Synctjik reports `native_backends: 1`, without `protection_backends`.
- `si-app-backup-pull.service` is loaded/inactive and still invokes
  `~/.local/bin/si-app-backup-pull`, whose installed shebang is Python.
- `si-app-backup-health.service` is loaded/failed with the same old helper.
  Its existing failure was not reset or diagnosed as part of migration.
- The local Drive service is not loaded, consistent with server-only policy.

The initial SSH probes failed: the configured LAN endpoint returned “No route
to host” and the Mullvad-excluded public route timed out. Later inspection found
that ordinary pinned-key SSH through Mullvad reaches Si. The current declared
backup SSH policy still uses the excluded route and must be reconciled before
live transfer acceptance. Remote backup `ExecStart` routing remains unverified.

The authorized Si Tailscale trial reached interactive login but did not complete
enrollment. Its ten-minute systemd rollback ran at 19:54:29 HKT on 2026-10-29;
`WantRunning=false` was verified afterward. nginx master/worker PIDs remained
unchanged, configuration checks passed, and 72 public HTTP observations retained
their expected responses. This verifies the observed trial and rollback, not
tailnet SSH or backup recovery. Fi's temporary process-specific VPN routing
change was restored. Full evidence and the retry procedure are in Dotjik's
`docs/tailscale-setup.md`.

## Remaining acceptance gates

1. Reconcile the declared SSH transport with the verified working route. Tailscale
   still requires interactive authorization and a fresh ten-minute guarded test;
   it is not required to use the already verified public SSH route.
2. Install reviewed capable user and root-owned Synctjik binaries, then perform
   the guarded Dotjik unit/file cutover with the shared Tjikup interface. A
   reviewed source build can be used on existing hosts while release publication
   and pin changes remain deferred. Existing active workers must finish first.
3. Inspect actual local/remote `ExecStart` and `ExecStopPost` routing, capability
   reports and removal of retired Python callers. Preserve timer enablement.
4. Validate one family at a time: Drive export/backup and isolated restore/hash;
   application snapshot, independent Fi copy and isolated restore/hash; then
   read-only SnapRAID identity/mount/lock/status checks. Real sync/scrub or alert
   delivery require their own explicit operational run, not a migration test.
5. Recheck the existing private-assets and Dotfiles LFS backup/restore routes and
   record success, snapshot, restored-content evidence and remaining exceptions.
   A service status or receipt alone does not prove recoverability.

Google Photos backup and an independent replica of the client backup histories
stored on Si remain separate coverage gaps, not implemented protection engines.
