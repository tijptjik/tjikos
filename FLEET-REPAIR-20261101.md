# Fleet repair, 1 November 2026

## Completed

- Published Si's preserved Dotjik and TjikOS review branches, then integrated both
  into main without resetting the shared dirty checkouts.
- Removed notification installer's private source-worktree override and producer
  reinstalls. Notification installation no longer replaces archive runtimes.
- Added exact-list SnapRAID review and one-use approval. The live review contains
  2,062 missing files. No removal approval or parity sync was executed.
- Delivered the incident to Fi through Notjik with **Review removals** and
  **APPROVE & SYNC**. Review fetches the current report through pinned SSH and
  opens a private, mode-0400 copy in Zed Preview. Native Zed SSH connections stalled
  at Starting proxy, so they are not required by the action.
- Installed `/usr/local/lib/synctjik/review` on Si as a separate helper, preserving
  the running `/usr/local/bin/synctjik` scrub (PID 1728764).
- Fixed Si Tasktjik's stale session group membership using both tasktjik and
  notjik groups. Its service completed successfully after the change.
- Restored Li-to-Si connectivity and verified Tasktjik HTTPS health and scheduled
  delivery. Li's explicit backup completed at 10:49 HKT, with Restic snapshot
  `98065820` covering documents, projects, resources and media. Dotfiles LFS retry
  also completed successfully. The earlier scheduled private backup was only a
  coordinated skip; the explicit backup establishes the new snapshot.
- Si home configuration applied: 65 changed and 699 unchanged entries in receipt
  `receipt-95E4v4`, followed by 3 updates and 761 unchanged in `receipt-2HtFN7`.
  Previously reviewed files were checked against the saved cutover snapshot before
  adoption. Changed configuration, retired Python files and replaced artwork were
  preserved under `~/.local/state/tjikup/fleet-20261101/preserved`.
- Verified Si storage mounts, nginx syntax, and `synctjik ready`.
- Fixed Tasktjik's call to a removed method, Runtjik's missing transitive lockfile
  entry, the workspace Bun lockfile, and the weekly unit's missing Bun paths.
  Runtjik now installs frozen browser-build dependencies and no longer checks
  for Python. Scoped Fish's bat alias to
  interactive shells so SSH cat remains byte-preserving.

- Li's complete weekly workflow passed at 11:17:38 HKT, including Runtjik.
  Its scheduled receipt records delivered=true, Tasktjik remains successful,
  no user units are failed, and both source checkouts are clean and match origin.

## Waiting or requiring follow-up

- Si's scrub remains active. `tjikup-si-config-converge-20261101.service` is queued
  after scrub and application backup. System file application correctly returned
  exit 75 while the maintenance lock was held; no lock was bypassed.
- The queued configuration run supersedes the failed historical cutover script;
  retain that old failure as evidence until the replacement actually succeeds.
- Si's canonical Synctjik runtime still needs the reviewed code after maintenance.
  Automatic approval review rejected even a queued, lock-protected replacement
  because it would replace the pinned worker path. No replacement job was created.
  Separate explicit approval is required before retrying that action.
- The user must inspect the removal report and choose APPROVE & SYNC if correct.
  Approval applies only to the exact removal-list/layout digest, expires after
  24 hours, and is consumed before parity changes. Mount, scrub and other safety
  gates remain enabled. It accepts missing files into parity, not deletion of
  existing source files.
- Si's old weekly failure remains outstanding until maintenance ends and a new
  complete weekly run succeeds.

## Verification

Dotjik validation passed 329 tests (10 skipped), including Fi/Li/Si template
rendering. Synctjik, Notjik transport and authority tests passed. Tasktjik's eight
library tests passed; its release build succeeded. The workspace
`bun run check` passed all 12 tasks and frozen Bun installation succeeded.
Runtjik check succeeded after its lockfile repair. The actual notification reached Fi with both actions, and
Zed opened the SSH-fetched report. The approval action was deliberately not
invoked against production parity.
