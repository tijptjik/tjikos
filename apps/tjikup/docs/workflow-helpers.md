# Native repository workflow helpers

`tjikup workflow --repo PATH COMMAND` runs repository policy directly in Rust.
Inside a recipe, `DOTJIK_REPO` supplies PATH. These commands do not invoke the
former Python orchestration scripts.

- `context`, `role`, `configure`: validate enrollment and resolve the same
  subscriptions, aliases, host/role filters and dependencies as the scheduler.
  Enrollment is required; there is no hostname-to-role fallback.
- `package check|skip-update|run NAME [-- COMMAND ...]`: use
  `tjikup/package-checks.json` and `native/tasks/data.json`. RPM collections use
  one fresh database query. Install and update selection remain separate.
- `files home|home-files|credentials|system check|plan|apply [--group NAME]`:
  select native manifests and provide context, preserve GTK's selected theme,
  decrypt system inputs into private temporary storage, and use the native
  deployment engine. System deployment uses sudo; home deployment does not.
- `assets fonts|icons --run-dir PATH`: aggregate related rendered Fish steps,
  retaining private diagnostics. Fonts only refresh their cache; no legacy
  font-folder consolidation runs.
- `bitwarden [--upgrade]`: install missing managed bws/bw tools without release
  lookups for already installed tools. Explicit upgrades discover releases,
  verify product identity and SHA-256 digests, and publish binaries atomically.
  A network lookup failure can retain a tested managed installation (exit 78).
  Invalid release identity or integrity is an error, not a deferred update.
- `services [--check] [--unit NAME] [--exclude-unit NAME]`: fingerprint managed
  runtimes and refresh only active daemons/timers. Never restart oneshot jobs.
  State checkpoints after each successful unit. `--check` changes no services.
- `activate bartjik|notjik`: coordinate activation with runtime refresh.
- `verify APP`: check the application's file group and refresh its active units.
- `repositories check|apply`: create declared missing checkouts without resetting
  or overwriting existing paths.
- `cache-install NAME --input PATH --output PATH -- COMMAND ...`: cache only
  matching source AND deployed bytes under a per-application lock.
- `capture [--dry-run]`: use `tjikup/capture.json` rules to capture live settings.
  Kinds are `copy`, `host-text`, `template-json` and `jsonc-scalars`. All rules
  are prepared before writes. JSONC comments, template expressions and control
  regions are retained; structural changes require a manual merge.

`files apply --adopt-identical` adopts unowned regular files only when bytes
match rendered desired bytes and the invoking user owns them. This happens
under the deployment lock. It cannot adopt symlinks or bypass drift on files
already recorded in state. Metadata changes retain ordinary backups/receipts.

Synctjik owns `synctjik migrate-schedules [--retire]`. It checks all old jobs
before disabling old timers and checks again afterwards. It never stops jobs.

Capture rules may declare `hosts` to limit shared preferences to named machines.
Excluded rules are skipped before reading either file. Host-specific text maps
can still capture each machine independently; server runs need not replace a
desktop session or application preferences with older server copies.

Maintenance preparation reports the number of recipes rendered and syntax
checked. It does not execute them. The former bootstrap batch is gone:
Bitwarden runs natively in Prerequisites, and font consolidation was removed.

## Public commands and ownership

`config home|system` provides plan/check/apply/source-path/edit. `git push|sync`
shares repository locks and never stages or commits. `ssh HOST` resolves pinned
fleet identity and executes the remote command once. Use each command's help.

`workflow bootstrap-archives` creates missing private archive transport and
policy. Synctjik owns enrollment, restoration, readiness and backup scheduling.
`workflow install NAME` loads native/installers.json, verifies release identity,
SHA-256 and installed bytes, and preserves newer installed versions.
`workflow desktop` owns Zed launcher preservation, icon override retirement and
duplicate-launcher cleanup. `timing take|accept` validates report transfers;
`progress` displays bounded transfer metrics without reading terminal input.

`tjikup system --help` lists native NSS, storage, provisioning, permissions,
media guard, Transmission cleanup and app-update operations. Host and root
checks use /etc/tjikup/maintenance.json. Backups and restoration belong in
Synctjik. Python applications and development tests retain their runtimes;
fleet orchestration has no Python prerequisite.

Validate with synthetic homes, mock commands and isolated file roots. Release
publication and bootstrap pins remain deferred; these source changes alone do
not update deployed binaries or published cold-bootstrap artifacts.

## Maintenance render cache

A native render batch can set `"cache": true` when its context and generated
scripts contain no credentials. The private cache under
`$XDG_CACHE_HOME/tjikup/templates` (default `~/.cache/tjikup/templates`) is keyed
by the runner executable, template definitions and bytes, merged runtime/data
context, and declared include bytes. There is no time-based refresh interval.

Cache hits verify each script's content hash, copy it into the isolated run
staging directory, and syntax-check it with the current validators. They do not
re-render the templates. Successful hits report SKIP with `N recipes cached`
when skipped tasks are shown. Failed validation never publishes a partial batch.
Batches with a Bitwarden provider always bypass this script cache; secrets use
the separate encrypted credential cache. Runtime context is still obtained on
each run so enrollment or policy changes invalidate cached scripts immediately.
