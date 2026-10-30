# Si backup and maintenance repair — 2026-10-30

## Application backup: repaired and verified

Installed published Synctjik `bootstrap.4` in the system runtime, preserving the
previous binary privately. The backup that formerly failed with `ENOENT` then
completed successfully at **14:18:19 HKT**. This release includes the staged
SQLite sidecar ordering fix.

- Snapshot: `9f078a50fa0925d90415cafe292bb245fd6cce398722e4c9d3eee0a61ed78fd3`.
- 25,797 files; 23,311,286,210 bytes.
- All 46 staged SQLite quick checks passed; MariaDB logical dump present.
- A small database was restored with Restic verification, matched the staged
  bytes and passed SQLite `quick_check`.
- The isolated restore retained destination SELinux labels using
  `--exclude-xattr security.selinux`; the initial probe failed attempting to
  remove a destination label. SELinux was not disabled. This proves sample
  content recovery, not a complete application or metadata restore rehearsal.
- Private restore receipt:
  `/var/lib/si-app-backup/restore-proof-20261030-qoBE61/receipt.json`.

## Drive backup: restarted, full-source policy retained

The original run failed at 03:45 because Google refused two flagged objects.
During this repair, live metadata checks found both objects absent from their
original locations. This session did not delete them or add exclusions.
Concurrent recovery work restarted the existing backup at **14:08:38 HKT** and
created `synctjik-drive-restore-validation-20261030-140838.service`.

That run and its user binary were left untouched. At the last check the backup
and validator were active; captured dependency error logs were empty. Restore
validation still waits for backup completion. No successful Drive receipt or
complete GSuite backup is claimed yet.

## Diagnostic release: published, staged, not substituted into a running job

Published signed `synctjik-v0.1.0-bootstrap.5`, source
`47f5a90230012c2ee600277ff14a331c237463ce`.
Static binary SHA-256:
`a672e8dff92a98f51211a4151cd7e094c09a8aa4ffa2035c85798df2fc6bce46`.

Drive now retains private rclone and Restic logs in its state directory instead
of losing dependency diagnostics when a process exits. Errors do not print log
contents. Failed/incomplete backups still block success receipts and retention.
No source-exclusion or abuse-acknowledgement option was added.

The release assets, provenance, static linking and capabilities were verified.
The binary is staged in Si's TjikOS checkout. Bootstrap pins and the public site
were updated; the deployed installer was byte-verified. Running Drive still uses
its original user binary; the successful application backup used `bootstrap.4`.

## Cutover: requeued against the new validation run

`tjikup-si-native-cutover-bootstrap7-20261030.service` is queued after the current
Drive backup and the new restore-validation service. It requires:

1. The new validator's durable `passed` / `complete` status, exit 0.
2. An inactive successful application-backup service and a success receipt newer
   than this repair's 14:06:14 HKT application-backup start.
3. Exact reviewed source revisions, clean tracked checkouts and staged checksums.
4. The original reviewed home/ownership baselines, which still matched.

Only then does it install the staged release and perform native home cutover.
The reviewed tool/policy revisions are respectively
`8a54f8ca7cb0d067cbbdf2948dc613db2acb0ed0` and
`971db4a6e5505ee6ef5533255d341b6fbe193032`.

Private logs and exit markers are `repair-run.log`, `repair-run.exit` and
`repair-run.finished` under
`~/.local/state/tjikup/si-native-cutover-20261029T151734Z/`.
The previous failed run and its evidence remain preserved.

## Transmission: repaired; approved removal completed

Reproduced the failure: `ProtectSystem=strict` made `/tmp` read-only, while native
subprocess capture requires temporary files. Added `PrivateTmp=true`, preserving
all other restrictions. Native deployment receipt:
`/var/lib/tjikup/files/receipt-8hnRDc/receipt.json`.

The real native dry-run under the corrected sandbox passed:
390 torrents scanned; 54 incomplete, 297 below the IPT seeding-age threshold,
27 unmanaged, and **12 eligible for removal**.

After explicit user approval, the fresh dry-run matched all 12 approved hashes.
The first execution exposed an additional native bug before any deletion:
serializing `OsStr` produced an object where the shared-file safety check expected
a filename string. Commit `2140d07` fixes this and adds a regression test; all
three targeted media tests passed. The checksum-verified release-mode build was
installed in Si's system runtime with the previous binary preserved. The fix
has since been published in signed Tjikup `bootstrap.7`, now installed and
checksum-verified in Si's user, system and weekly runtime locations. The queued
cutover's reviewed runtime pin was updated and its waiting job replaced without
changing its backup/restore gates or interrupting running backup workers.

At **14:54 HKT**, cleanup succeeded and verified removal of the exact 12 approved
torrents and their downloaded files, representing 55,654,194,885 logical bytes
(not a measurement of reclaimed disk space). Receipts matched the approved hash
list. The follow-up dry-run scanned 379 torrents with no removal candidates.
The temporary approval guards were removed and the previously enabled cleanup
timer restored; its next run is 31 October at 04:36 HKT.

Private approval, recheck and removal receipts are preserved under
`~/.local/state/tjikup/si-backup-repair-20261030/`, including
`transmission-approved-removed.jsonl`. The normal audit remains at
`/var/lib/si-transmission-cleanup/removals.jsonl`.

## Validation

- Synctjik: 24 Rust tests passed, one ignored; all 76 compatibility/protection
  tests passed. Signed release CI passed.
- Policy: 322 tests, nine skipped, passed; fleet template validation passed.
- Bootstrap: ten checks passed and public installer byte verification passed.
- Live proof: sandbox reproduction/fix, Transmission dry-run, successful
  application snapshot and sample database restore. Drive completion and full
  restore validation remain outstanding.
