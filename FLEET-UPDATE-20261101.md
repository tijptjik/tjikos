# Li and Si update — 1 November 2026

## Repository work preserved

Li's TjikOS, Dotjik and legacy system checkouts were clean on inspection.
Si's main TjikOS checkout was clean. Its dirty Dotjik policy and separate
Notjik source worktree were committed without touching Fi's concurrent work.

| Repository on Si | Commit | Change |
| --- | --- | --- |
| Dotjik | `e50e443` | Disk-backed temporary storage |
| Dotjik | `989492d` | Notjik authority, notification routing and backup policy |
| TjikOS Notjik worktree | `59f45ad` | Notification authority and durable transport |
| TjikOS Notjik worktree | `607b805` | Desktop and fleet producer integration |

The source worktree is on `review/si-notjik-20261101`. Policy commits are on Si's
local main and `review/si-policy-20261101`. Publication was rejected by automatic approval
review pending explicit permission; no remote push was performed. The feature
is preserved, not declared fully integrated or ready for fleet-wide rollout.

Validation: native policy rendering and all 328 policy tests passed (12 skipped).
Notjik transport and authority Rust tests passed. This does not establish live
end-to-end notification delivery or all cross-application regression coverage.

## Update runs

Tjikup package-update scopes completed successfully (exit 0, verification
passed), with explicit PATH and notifications disabled:

- Li: `tjikup-li-updates-20261101.service`.
- Si: `tjikup-si-updates-20261101.service`.

Li's private-folder sync subsequently completed and full configuration
convergence passed (exit 0) as `tjikup-li-converge-20261101.service`.
Its first run stopped at a Herdr session byte-level conflict. Live JSON and the
rendered template were semantically identical (six workspaces). The original
was preserved privately and formatting normalized; the retry passed that gate.
Si's full configuration convergence is deferred while its SnapRAID scrub runs.
Si's pending Notjik installer builds and replaces
Synctjik, so it must not execute beneath a pinned active worker. Repository
synchronization was skipped while preserving and reviewing the dirty work.

## Outstanding work

- Finish Si's configuration convergence and application updates after the scrub;
  its package run safely skipped application updates because another maintenance
  operation holds their lock.
- Integrate and validate the preserved Notjik work, including replacing the
  hard-coded private-worktree dependency and guarding active Synctjik workers.
- Review Si's 2,062 proposed SnapRAID removals; its limit is 200. The safety guard
  remains enabled and no forced synchronization was attempted.
- Si's Drive restore validator passed on 30 October at 22:48 HKT. The subsequent
  native cutover exited 1; its historical runtime/source pins must be reconciled
  with later deployments before retry. Backup completion is no longer the blocker.
- Diagnose Tasktjik notification failures on both hosts: Li timed out reaching
  the authority; Si reported a local socket connection failure. Si's local health
  endpoint now returns `status:ok`, but a fresh HTTPS health request from Li
  timed out. CLI verification alone does not prove scheduled delivery works.
- Li's scheduled private backup failed because Si could not be reached through
  pinned SSH; its Dotfiles LFS backup collided with another archive operation.
  Its sync has now finished; retry backups and require new snapshot/receipt proof.
- Li's weekly run failed a Bitwarden lookup. Si's weekly run failed a repository
  pull while local policy was dirty. Verify their next complete runs after the
  relevant repairs; do not clear historical failures as proof of success.
