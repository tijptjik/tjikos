# Tjikup cutover checkpoints

Updated 2026-10-29 after the Fi application review, Transmission repair and Si audit.

## 1. Fi: native deployment and desktop acceptance

Native Tjikup and Synctjik are installed. Capability checks confirm native
workflows, commands, installers, maintenance and protection engines. The home,
system, Git and fleet-SSH shortcuts delegate to Tjikup.

The reviewed home-file plan was applied with the normal Synctjik protection
guard. Its private rollback receipt is under
`~/.local/state/tjikup/files/home/receipt-grbSYN/receipt.json`.

Completed in this checkpoint:

- Removed the unsupported Kitty resize option and repaired prompt navigation
  shortcuts (Ctrl+Shift+Z/X). The installed Kitty parser reports no config errors.
- Retired the Monochrome launcher: its PWA ID exists in neither the installed
  registry nor the managed registry.
- Removed the old Codex checkout trust entry from both source and live config,
  preventing the capture step from restoring it.
- Stopped the standalone Synctjik installer from recreating the retired
  deployment-backup command.
- Removed the unused legacy deployment RPM after checking that the transaction
  removed exactly one package. Retired the old root wrapper, checkout aliases,
  Fisher plugin and obsolete Fish search paths. Local recovery copies are in
  `~/.local/state/tjikup/cutover-fi-20261029/`.
- Retired obsolete home helper files through the native manifest. Existing
  credential recovery material and backup/archive workers were preserved.
- Added Fish completions for all seven CLI apps, both control commands and the
  managed wrappers. App installers now install them; Fi's normal Fish shell
  autoloads the installed files. Eight completion tests and seven installer tests
  pass without invoking application workloads.
- Repaired Transmission 4.1.3's favicon-worker self-join crash with a pinned,
  GTK-only build. The original lifetime regression aborts and the patched one
  passes. Both a fresh synthetic profile and the backed-up real profile ran
  with transfers paused and exited successfully. The real window was observed
  mapped and `/proc/PID/exe` identified the repaired binary. Desktop menu actions
  explicitly select it. This was a brief launch check; user visual acceptance
  and sustained use remain separate. See Dotjik's `docs/transmission-desktop.md`
  for installation, evidence and rollback. Li and Si were not updated.
- Fixed the subsequent Fi failure on application-written `mimeapps.list` and
  Transmission settings. These are now seeded only when absent; runtime
  preferences survive normal reapplication. The live home/system configuration
  run completed with `ALL SYSTEMS GO`. Native capture also supports host filters,
  preserving Fi/Li desktop captures while excluding Si's older copies.

### Application review

| Workspace | Applications exercised |
| --- | --- |
| 14 | Zed stable, Obsidian, Strata, Sublime, Markdown Viewer |
| 15 | Brave, Chrome, Chrome Canary |
| 16 | Files, Qt6 settings, Fcitx settings, Solaar, audio settings |
| 18 | VLC, Transmission diagnostics |
| 19 | Kitty/Fish, Micro, Bottom, Lazygit, using synthetic inputs |
| 6 | Gmail, Photos, Calendar, Drizzle Studio, Codex LB PWAs |

Windows were observed for the ordinary application launch batch. Several later
exited normally. Existing Zen, Spotify, Herdr/Kitty and desktop services were
preserved. Runtjik was shown and its config check passed; Notjik received one
synthetic local notification. Mullvad connected to its daemon and retained its
start-minimized preference, but no mapped window was confirmed.

Open acceptance items:

- Visual confirmation from the user is still needed. Successful launches and
  parser checks do not establish that every appearance setting is correct.
- A concurrent icon workstream restored legacy asset names with new-name
  aliases. Those edits were preserved and excluded from this thread's commits.
  Reconcile the naming decision before calling the terminology cleanup complete.

Validation: workspace `bun run build` passed with a writable temporary Go cache;
8 completion tests and 7 installer tests passed; Dotjik validation passed 321 tests with 9 skipped,
including Fi/Li/Si template rendering. The first validation encountered icon
files changing concurrently; the completed second run passed.

## 2. Li: native cutover and updates complete; fresh backup proof pending

Li is reachable through the pinned native fleet resolver. Its paused rebase and
live settings were reconciled, including a separate Li Herdr session template.
Native runners and completions are installed. Home, credential and system file
deployments completed with rollback receipts, and all three final checks passed.

After explicit user approval, package upgrades and configuration convergence both
finished with ALL SYSTEMS GO. Docker's native nftables backend and explicit
forwarding are configured; Docker and Tailscale are active. Synctjik is enrolled
as a laptop follower of Fi with eight restored shared collections; readiness
passes and all three native backup/sync timers are enabled. The real repository
directory is now Dotjik, with the old name retained as a compatibility symlink.

A fresh manual backup verification is still blocked by automatic approval review,
which requires user confirmation naming the four private folders and their
existing encrypted Restic destination on Si. No fresh backup success is claimed.
See [LI-CUTOVER-AUDIT.md](LI-CUTOVER-AUDIT.md) for the exact approval request,
validation limits, recovery evidence and the unresolved 82-second Plymouth stall.

## 3. Si: system cut over; home queued behind backup

The Telegram edits were preserved in verified private archives and recovery
commits, then reconciled with their equivalent native policy. Native Tjikup is
installed for user and system commands; the capable native Synctjik binaries
installed by the parallel protection-validation work were retained.

The guarded system deployment completed with a rollback receipt. Native unit
routes, nginx validation and mounted storage passed; timer enablement and the
running container IDs were preserved. The subsequent home run correctly stopped
at the active Drive backup lock. `tjikup-si-native-cutover.service` is queued
after that backup and its existing restore-validation job, with approved-source
and live-file checks before the remaining apply. Package upgrades and Git
publication are excluded from the queued run.

See [SI-CUTOVER-AUDIT.md](SI-CUTOVER-AUDIT.md) for the preservation commits,
receipts, queued-run logs and remaining acceptance gates. Home convergence and
legacy-entrypoint retirement are still pending; do not report the queued run as
successful before its exit status and receipts exist.

Fi's old backup-health failure predates the native unit deployment. Its current
unit calls `synctjik app-pull health`. A fresh successful health check and backup
receipt are still needed; the old failure was not cleared to imply success.

Release publication and pin advancement remain deferred.
