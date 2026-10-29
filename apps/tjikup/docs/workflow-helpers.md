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

Maintenance preparation reports the number of recipes rendered and syntax
checked. It does not execute them. The former bootstrap batch is gone:
Bitwarden runs natively in Prerequisites, and font consolidation was removed.

## Remaining Python boundary

The recipe graph's orchestration is native. Some rendered application installers
still invoke Python: Obsidian, Zed launcher preservation, Rose Punk theme
installation, and embedded upstream release installers. Si's NSS/storage,
SnapRAID and application-update utilities also remain Python. These are still
migration work, so the Python prerequisite is retained. Python-based application
runtimes and development/test tools are a separate dependency category.

No full package installation, service refresh or backup run is required to
validate these helpers. Use synthetic homes, mocked commands, native tests,
read-only capture previews and maintenance-template syntax checks first.
