# tjikup

A Rust runner for declarative, resource-aware recipes. The binary has no Python
runtime dependency. Recipes may invoke any executable. Chezmoi and Chezetc are
optional configuration-manager adapters, not the task scheduler.

Build with `bun run build` (or `bash scripts/build.sh`). Run `bun run check` for
formatting and Clippy, and `bun run --cwd apps/tjikup test` for isolated behavioral
tests. Install with `bash apps/tjikup/scripts/install.sh [prefix]`; replacement is
atomic, so running processes retain their executable. The installer downloads
the static GitHub Release executable pinned in `apps/tjikos-sh/release.json` and
verifies its SHA-256 using `curl` and `jq`. Rust/Cargo is required only for an
explicit source build, not for installation or ordinary updates.

Native file deployment is available through `tjikup files`; see
[Native file and template management](docs/native-files.md) for manifests,
conflict handling, isolated verification and recovery. Existing Chezmoi adapters
and native MiniJinja maintenance templates are supported.

## Configuration

The default is `$XDG_CONFIG_HOME/tjikup/config.json`, falling back to
`~/.config/tjikup/config.json`. `TJIKUP_CONFIG` and `--config` override it.
Version 1 Python workflows must be migrated before installing this version.

```json
{
  "version": 2,
  "title": "My workstation",
  "jobs": 4,
  "prerequisites": [
    {"id": "require-git", "check": ["git", "--version"]}
  ],
  "recipes": [
    {
      "id": "tool",
      "check": ["my-installer", "--check"],
      "apply": ["my-installer", "--apply"],
      "resources": ["packages"]
    },
    {
      "id": "services",
      "needs": ["tool"],
      "apply": ["my-service-refresh"],
      "resources": ["user-systemd"]
    }
  ]
}
```

A recipe check exits **0** when current, **1** when apply is needed, and **2 or
higher** on error. An error never invokes apply. Without a check, apply runs each
time. A check-only recipe is a requirement: every nonzero exit fails. Top-level
`prerequisites` must contain check-only recipes and gate every ordinary recipe.
Checks are executable policy: they must implement their own read-only behavior.

`needs` lists recipe IDs; `resources` lists exclusive resource names. Checks and
apply hold the same resource lease. Unchanged prerequisites satisfy dependents.
Independent tasks run up to `jobs` (1–256, overridable with `--jobs`).
`interactive: true` gives a recipe exclusive use of the terminal and stdin.
Other commands have null stdin and private logs. A failure stops new dispatch,
drains already-running peers and blocks pending work. Nothing rolls back completed
external mutations. SIGINT/SIGTERM cancels owned process groups and exits 130.
Each command has a deadline, `timeout` seconds (default 300), including checks.

Commands are argv arrays, never implicit shell expressions. Use an explicit
shell script for shell behavior. `cwd` defaults to the declaring document's
folder. `env` can be set globally or per recipe. Values support `{config_dir}`,
`~/`, `$NAME`, `${NAME}` and `$$` for a literal dollar. Configuration symlinks
resolve to the source file's directory. `{run_dir}` names private per-run scratch
storage, which is removed on exit. Environment values are resolved independent of
JSON key ordering; unknown variables and cycles fail validation.

`includes` imports other version-2 documents. Each include has its own
`{config_dir}` and inherits the parent's environment. IDs and manager names must
be unique across the graph. Root `flags` maps CLI switches to environment names,
for example `"flags": {"upgrade": "UPGRADE"}`; the runner supplies `1` or `0`.
Optional `label` controls display. `skip_codes` marks selected apply exit codes
unchanged; `stop_codes` stops subsequent dispatch with an attention outcome and
exit zero. Exit-code mappings never apply to checks.

`--check` validates the entire graph without running commands. `--plan` and
`--dry-run` print dependencies/resources without executing checks, preview
commands, templates, Git or mutators, and create no runtime state. The optional
`preview` field is reserved metadata and is never executed by these modes.

All loaded configuration files are snapshotted. If a pull changes a recipe
configuration, remaining work stops and the user must rerun to load the new graph.
The runner never mixes a stale graph with a newly pulled recipe definition.

## Git and scoped autocommit

Autocommit is an explicit recipe action:

```json
{
  "id": "capture-commit",
  "needs": ["capture"],
  "autocommit": {
    "repository": "{config_dir}",
    "files": ["home/dot_config/editor/settings.json.tmpl"],
    "message": "sync: capture editor settings"
  }
}
```

Only exact, already-tracked files are accepted, including tracked deletions.
After moving capture paths during a source migration, review and commit the new
files before running the workflow. Untracked capture paths stop autocommit before
any commit or index change; the status shows the count and the private receipt
lists the paths.
Directories, path traversal and unfinished Git operations fail. Paths use Git's
literal pathspec mode. `git commit --only` preserves unrelated staged changes;
there is no blanket add, stash, reset or force push. Every change in an explicitly
listed file is authorized by that policy, so list only files intended for capture.
Git signing and hooks retain normal repository behavior.

The optional `git` action takes `repository` and `operation`: `pull-rebase`,
`pull-ff` or `push`. All require a clean worktree, attached branch and upstream.
`pull-ff` fetches and fast-forwards without discarding locally ahead commits;
divergence fails. `push` refuses unresolved upstream commits. Git operations use
per-worktree locks shared with the fleet shortcuts. Optional root `locks` lists
additional existing-parent lock paths; a default configuration lock is always held.
Locks are nonblocking and held for the complete run.

## Native maintenance templates

The fleet uses MiniJinja for maintenance scripts, without a configuration-manager
process. A render recipe declares an input manifest, an optional explicit context
command returning a JSON object, and the scripts to prepare:

```json
{
  "id": "prepare",
  "render": {
    "native": "{config_dir}/templates/inputs.json",
    "context_command": ["python3", "{config_dir}/scripts/context.py"],
    "templates": [{
      "name": "setup.fish",
      "source": "{config_dir}/templates/setup.fish.j2",
      "syntax": ["fish", "--no-config", "--no-execute", "{script}"]
    }]
  }
}
```

The input manifest has `version: 1`, optional `context`, `context_files` (JSON
objects), `includes` (name-to-relative-file mapping), and `bitwarden` using the
same provider configuration as native file manifests. Context merges in that
order, followed by the context command's values. Includes cannot escape the
manifest directory. `include(name)` returns the declared file literally; templates
cannot execute commands or read arbitrary files. Undefined variables fail.

`.j2` sources render natively; other sources copy literally. Filters include
`fish_quote`, `json_quote`, `b64enc`, and `sha256`. Secret values are available
through `secret(uuid)` only with an explicit Bitwarden provider. Choose quoting
for the destination language. Scripts are private temporary files; validator
output is suppressed because it may quote secret-bearing source lines.
The complete batch must render and pass syntax checks before its scripts are
published to dependent recipes. A consumer uses `{run_dir}/setup.fish` in `apply`.
Prepared scripts and temporary validator diagnostics are removed after the run.

The optional legacy `manager` render and `files` adapters remain available for
external configurations. A render batch must select exactly one of `native` or
`manager`; the consolidated dotfiles graph declares no legacy managers.
Privileged tasks still require explicit `sudo` argv and environment policy.

## Reporting and notifications

Repository rows show completed commit counts in the right-hand status column:
`↓3 ↑2` means three upstream commits pulled and two commits pushed. Rebasing
local commits does not inflate the pull count. Current repositories show
`↓0 ↑0`; skipped, pending or failed operations retain `–` instead of a claimed
count. Counts are also recorded as `git_commits` on successful Git recipe receipts.

Private per-recipe logs and `receipt.json` live under
`$XDG_STATE_HOME/tjikup/logs/run-*` (default `~/.local/state`). Receipts contain
outcomes, execution and dependency/resource/worker wait times; they do not copy
commands, credentials, environment values or status notes. Interactive tools keep
foreground stdin and native stderr; stdout is logged, with structured status lines
rendered once by the parent. Unterminated prompts pass through immediately. Rendered scripts and
captured template output are temporary and removed on completion or failure.

Local completion cards call `notjikctl send` directly, with `notify-send` fallback.
`TJIKUP_NOTIFY=0` disables delivery. Direct Kitty SSH sessions send display-only
OSC 99 metadata. The small Kitty Python hook forwards that metadata to
`tjikup notify --stdin`; no Python imports from the runner are required. The
receiver validates the display schema and never accepts remote action commands.
Headless and unsupported remote terminals stay silent.


## Terminal presentation

The native Rust renderer preserves the original rainbow splash, coloured section
headings, repository PULL/PUSH cells, right-aligned notes and completion banner.
Kitty and Herdr use the existing 32px sprite catalogue with scrolling Unicode
placeholders. Results use the spacious two-row layout by default.
Set `TJIKUP_COMPACT=1` to use one row, scaling icons to fit the cell height. Other terminals
use configured glyphs, and redirected output
is plain text. Spinner updates only patch repository text, leaving images intact.

Root `presentation` configures `splash` (boolean), `url`, `icons` (a path to the
`zlib-base64-rgba` sprite JSON), `icon_aliases` (report label to icon key),
`glyphs` (label-to-glyph map), and `titles`
(label-to-display-name map). These are optional; fleet identities stay in the
workflow configuration rather than the runner.

Set `presentation.links` to an array of `{ "title": "…", "url": "…" }` objects
to replace the single title/URL with multiple project links. Links appear side
by side when the terminal is wide enough and stack on narrower terminals.
The splash and project links are centered within the terminal width.

Subtask rows use their own known service icon, an explicit `icon_aliases` mapping,
or their recipe's `display.icon` key. If no explicit key is set, the recipe label
is used as the fallback. Thus “Tailnet connection” can inherit the Tailscale
icon without coupling artwork to changing status text. The same resolution
applies to both sprite and glyph rendering.

Each recipe may supply `display`:

```json
{"section":"Recipes", "verb":"UPDATE", "reports":true}
```

`reports: true` shows changed/warning/failure hook reports instead of a generic
successful completion. Empty role guards and SKIP/no-change reports stay quiet.
Run `tjikup --show-skips` to also display settled skip/check reports and their
reasons. Successful checks show “already current”; unmet recipe conditions show
“condition not met”. Prerequisites show “prerequisite satisfied”. A silent recipe
that supplies no report is labelled “no status reported”, rather than inventing a
skip reason. This option changes presentation only and combines with workflow
flags such as `--skip-repo-sync`. A later skipped substep never replaces an actual
update to the same subject. Skip rows use the dim palette.

`quiet: true` hides successful internal preparation in the default view. Failures always remain
visible. Repository recipes use `{"repository":"dotjik","operation":"pull"}`
or `"push"` to share one row. Headings appear only when there is a result to show.

Hooks append `verb<TAB>icon<TAB>subject<TAB>note` to `TJIKUP_REPORT_FILE`.
Structured stdout status lines are a fallback. An update followed by a skipped
substep remains visible; warning and failure records are never hidden. The
runner sets `TJIKUP_STATUS_LINES`, `TJIKUP_STATUS_SECTIONS`, `TJIKUP_NO_SECTIONS`
and `TJIKUP_NO_SPIN` for child commands. Explicit privileged environment
allowlists must include the corresponding values where needed.

`TJIKUP_COLOR=0/1` overrides automatic colour detection (`NO_COLOR` is honoured).
`TJIKUP_ICONS=0` disables icons; `TJIKUP_KITTY_ICONS=0` selects glyph fallback.
`TJIKUP_NO_SPIN=1` disables animation while retaining settled styling.
Receipts are saved for diagnostics; their paths are not printed in the terminal footer.

Use `tjikup --demo changed`, `--demo unchanged`, or `--demo attention` to preview
synthetic output. Add `--show-skips` to preview explicit checks and skips, for
example `tjikup --demo unchanged --show-skips`. These do not load your workflow, execute tasks or send
notifications, and never perform Git, package, service or font operations.

## New devices and subscriptions

`tjikup new device` asks for desktop/laptop/server and optional recipes, saves a
private local profile, and shows required dependencies before saving. It never
runs the selected tasks. See [the Fedora bootstrap guide](../tjikos-sh/README.md)
for the cold path, credential unlock, role boundaries, and publication steps.
Use `tjikup new device --replace` to revise a profile and `tjikup --plan` to review.

A root `device_file` opts into profiles; `legacy_hosts` lists explicitly retained
unenrolled hosts. Recipes declare `optional`, `roles`, and `hosts`. `needs` pulls
in prerequisites; `after` only orders recipes already selected. Removing a
subscription does not uninstall or delete previously managed files.

### Installation, configuration and updates

Recipes may declare `operation`: `install`, `configure`, `update`, or `prepare`.
Configuration recipes also declare `scope`: `home` or `system`. Use `needs` for
required dependencies and `after` for ordering only. Application configuration
should need its own installer, whose `check` tests local presence without fetching
release metadata. All updates wait for the selected non-update work to finish.

- `tjikup`: install missing applications, configure them, then update packages.
- `tjikup --skip-package-updates`: install and configure without updating.
- `tjikup --apply-dotfiles`: home configuration and associated reloads.
- `tjikup --apply-sysfiles`: system configuration and associated reloads.
- `tjikup --apply-package-updates`: update installed applications.

The three `--apply-*` flags combine. Explicit selectors exclude uncategorized
capture/Git recipes. An installer needed by selected configuration becomes a
local presence check; if missing, the run fails with an installation instruction
and never calls the installer. Preparation dependencies remain available. An
unselected configuration dependency is rejected rather than silently changing
another scope. `--plan`, `--dry-run`, and `--check` never execute any commands.

Optional `application` and `ecosystem` strings describe policy ownership.
`part_of` makes companion configure/update recipes inherit their parent's device
subscription without making that parent an execution dependency. The dotfiles
repository owns the checks, install/update commands, and configuration groups.

`tjikup sync <command>` forwards arguments and exit status to `synctjik`,
including `tjikup sync status` and `tjikup sync setup`.

### Grouped results and device diagnostics

`presentation.grouped_results: true` executes one section at a time, with
parallel workers inside the section and results shown immediately in completion
order. There is no alphabetical buffering. Activity timing uses the right-hand
status column. Dependencies and resource locks remain authoritative: a task from
a later section can run early when it is a prerequisite of the current section;
only that prerequisite's row waits for its own section. This handles cross-section
dependency chains without introducing cycles or stalling the workflow. Failed
and blocked rows retain their sections. Repository matrix updates remain live.

`TJIKUP_SECRET_TIMING=1` reports aggregate Bitwarden request counts and provider
time for troubleshooting; it never prints secret values or identifiers. Secret
lookups use the optional encrypted cache described below; cache misses are
fetched sequentially and repeated IDs are also cached in memory per render.

Enroll machines with `tjikup new device` to choose their role and explicit
recipe subscriptions. `--show-skips` never adds
excluded recipes. `--show-skips-and-other-devices` lists excluded recipes as
informational rows and does not execute them.

### Encrypted Bitwarden cache

Enable persistent caching in a native manifest or template-input document with
`"bitwarden": {"command": ["bws"], "access_token_file": "...", "cache": {}}`.
The cache is opt-in; Dotjik enables it for its user manifests. It has **no TTL,
interval refresh, or background job**. Missing entries are fetched individually;
normal renders reuse existing values, even when offline.

Ciphertext lives in `$XDG_CACHE_HOME/tjikup/credentials` (normally
`~/.cache/tjikup/credentials`), in owner-only directories/files. ChaCha20-Poly1305
authenticates each entry against its secret ID, provider command and access-token
scope. A random encryption key is stored through `secret-tool` in the user's OS
Secret Service (oo7 on Fi), never beside the cache. The access-token file is still
required locally; changing that token isolates the old cache. Cache corruption
causes a fresh provider lookup, not acceptance of unverified data. Per-secret
locks coalesce concurrent misses and refreshes. If Secret Service is unavailable,
Tjikup warns and uses Bitwarden directly; it never creates a plaintext fallback.

Refresh referenced credentials explicitly:

```sh
tjikup --apply-dotfiles --refresh-secrets
# Or refresh only a standalone manifest:
tjikup files apply --manifest credentials.json --state-dir /absolute/state --refresh-secrets
```

A refresh is fetched once per secret per invocation, including dependent child
renders. Successful values replace ciphertext atomically; failed refreshes fail
the operation rather than silently using stale values. Ordinary checks/plans do
not deploy files, but may populate this encrypted cache when enabled. Timing
with `TJIKUP_SECRET_TIMING=1` reports provider requests and cache hits without
values or identifiers. `cache.directory` can select an absolute cache location;
`cache.keyring_command` selects a Secret Service CLI compatible with
`secret-tool lookup/store` (primarily for integration tests).
