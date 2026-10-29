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
8 completion tests and 7 installer tests passed; Dotjik validation passed 320 tests with 9 skipped,
including Fi/Li/Si template rendering. The first validation encountered icon
files changing concurrently; the completed second run passed.

## 2. Li: pending connectivity

The native fleet resolver found no reachable, verified SSH address. Resume when
Li is awake and connected. Use the pinned host identity, inspect its dirty state
and running workers, then install capable native runners and review the home
and system plans separately. Repeat the desktop acceptance check before retiring
the old installation. Do not substitute an unverified remembered IP address.

## 3. Si: reconcile before applying

The trusted public SSH endpoint is reachable. Its installed Tjikup is still the
Python runner; neither the home nor system Synctjik executable was present.
The Dotjik checkout is at `a607309`, with a modified Docker Compose template
and untracked Telegram archive files; the separate system checkout also has
an nginx-site edit. The read-only comparison found all these additions already
represented in native policy. See [SI-CUTOVER-AUDIT.md](SI-CUTOVER-AUDIT.md) for
the file-by-file findings and server-specific gates. Preserve that work before
updating the checkouts; do not run a blanket apply against them.

Next steps, in order:

1. Inventory active archive/backup jobs, locks, receipts and current server
   policy. Do not restart or replace pinned workers.
2. Preserve and reconcile the dirty checkout with the current native policy.
3. Install matching native Tjikup and Synctjik builds for both user and system
   entrypoints; verify protection capabilities before any protected-file apply.
4. Review home and system plans separately, apply through the protection guard,
   then verify native unit commands, schedules and fresh recovery evidence.
5. Retire legacy entrypoints only after the native acceptance checks pass.

Fi's old backup-health failure predates the native unit deployment. Its current
unit calls `synctjik app-pull health`. A fresh successful health check and backup
receipt are still needed; the old failure was not cleared to imply success.

Release publication and pin advancement remain deferred. No remote files were
changed during this checkpoint.
