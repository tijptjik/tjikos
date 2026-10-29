# Protection migration evidence — 2026-10-29

Native implementation and synthetic recovery are complete. Live acceptance is
**not complete**: Si's Drive service now uses native Synctjik and its first native
backup is running; completed backup and isolated restore evidence are still
pending. The other Si protection families still need their guarded cutovers.
Fi has the capable binary and native copy/health unit routing. Release publication
and pin changes remain deferred. No real parity operation, notification delivery,
or backup schedule activation was performed by this work. The separate Tailscale
trial below did not exercise a backup engine.

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

Initial local inspection, before the subsequent installer repair, found:

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
live transfer acceptance.

At 22:32 HKT, Si still had neither the user nor root-owned Synctjik binary;
its loaded/failed Drive service still invoked the old helper. Fi's installed
binary reported `protection_backends: 1`, and its copy/health units invoked
`synctjik app-pull copy|health`. The copy unit was inactive and the health unit
remained failed; these observations are not proof of a successful backup.

The authorized Si Tailscale trial reached interactive login but did not complete
enrollment. Its ten-minute systemd rollback ran at 19:54:29 HKT on 2026-10-29;
`WantRunning=false` was verified afterward. nginx master/worker PIDs remained
unchanged, configuration checks passed, and 72 public HTTP observations retained
their expected responses. This verifies the observed trial and rollback, not
tailnet SSH or backup recovery. Fi's temporary process-specific VPN routing
change was restored. Later browser approval completed Si's enrollment, but also
brought it online after the second login timer had expired. It was disconnected
before a fresh guarded connection test. Two fresh pinned-key IPv6 SSH sessions
passed, with public SSH and nginx still available. The new timer disconnected
Si at 22:35:22; `Stopped`, `WantRunning=false`, peer offline state, and failure of
a fresh tailnet SSH connection were verified. Public SSH and the four checked
HTTP routes still worked, with unchanged nginx master/worker PIDs. Si remains
enrolled but disconnected, and Fi's temporary routing adjustment was restored.
Full evidence and the late-approval limitation are in Dotjik's
`docs/tailscale-setup.md`. No backup/restore was run during either network test.

## Live Drive cutover and backup — 23:02 HKT

Ordinary pinned-key public SSH reached Si with Tailscale still disconnected.
Both configured Google source trees were accessible. The existing Restic
repository contained one archive snapshot, but the old worker had failed with
incomplete reads and had no success receipt. That snapshot is not accepted as a
complete backup; no GSuite snapshot was present at preflight.

The reviewed release build ran on Si and reported `protection_backends: 1`.
Identical binaries were installed at `~/.local/bin/synctjik` and the root-owned
`/usr/local/bin/synctjik`. The legacy Drive operation lock and worker guard covered
the file cutover and user-manager reload. A staged native Tjikup adopted the four
verified existing Drive files under the canonical `home-files-si` manifest ID,
then applied the selected current Dotjik entries: new policy, native service
command and shell delegation. The timer and failure handler were identical.
Tjikup retained the replaced files and metadata in a private recovery receipt;
its subsequent file check reported all five selected files unchanged.

Effective `ExecStart` is `~/.local/bin/synctjik drive backup`; the compatibility
command also passed native policy validation. The Drive timer remained disabled
and inactive. nginx remained active with the same master PID and start time.
Neither Si configuration checkout, other protection services, nor pinned archive
workers were changed. The native Tjikup helper was staged solely for this file
deployment; Si's general Tjikup installation was not replaced.

The manual Drive service invocation started at 23:02:06 HKT. It is independent of
the SSH session. A separate one-off systemd validation job waits for that exact
invocation to finish successfully and publish a new receipt. Only then does it
select the new snapshots, check free space, and invoke native `drive restore`
with full Restic verification into new owner-only storage-pool directories. It
also compares representative restored hashes against independent `restic dump`
reads and checks representative exported Office containers. This job does not
enable a timer, overwrite live data, or proceed after backup failure.
The verifier has reduced CPU/I/O priority. While the backup was running, the
native cutover guard returned 75 without changing files, confirming that the
active worker's legacy lock blocks another migration.

At 23:11 HKT the archive backup was progressing, reusing its previous snapshot,
with no captured rclone or Restic errors. GSuite's read-only inventory counted
5,023 files and about 5.2 GB of known sizes, including 205 files with unknown
export sizes. The validation job was still waiting: backup completion, repository
integrity and retention, both new snapshots, and restored-content evidence remain
pending. Detailed diagnostics and runtime evidence are private on Si, outside
both repos. Public home/read/listen routes returned HTTP 200 and music returned
HTTP 302; nginx's master remained unchanged and Tailscale reported `Stopped`.

## Remaining acceptance gates

1. Reconcile the declared SSH transport with the verified working route. Si's
   tailnet enrollment and guarded connection test passed, but it is intentionally
   disconnected. Persistent tailnet use also needs Fi's coordination-route policy
   reconciled. The verified public SSH route can be used independently.
2. Si's capable user/root binaries and Drive-only cutover are installed. Perform
   the remaining application and SnapRAID Dotjik unit/file cutovers with the
   shared Tjikup interface. Release publication and pin changes remain deferred.
   Existing active workers must finish first.
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
