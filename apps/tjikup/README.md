# tjikup

A configurable script runner with ordered stages, bounded parallel groups,
aligned status rows, elapsed times, failure diagnostics and desktop completion
notifications. Python 3.10+ is its only required dependency. `notify-send` is
optional. No configuration manager, fleet path, repository or credentials are
built into the runner.

## Install and run

```sh
bash apps/tjikup/scripts/install.sh
# Optional installation prefix (bin/ and share/ underneath):
bash apps/tjikup/scripts/install.sh /tmp/tjikup-install

tjikup --config /path/to/workflow.json --check
tjikup --config /path/to/workflow.json --dry-run
tjikup --config /path/to/workflow.json
```

The installer builds a self-contained Python zipapp. It does not require Bun,
Go, pip or another tooltjik application. In the monorepo, `bun run build` and
`bun run check` include this workspace; `bun run --cwd apps/tjikup test` runs
its behavior tests. Installation replaces the executable atomically.

The default configuration is `$XDG_CONFIG_HOME/tjikup/config.json` (normally
`~/.config/tjikup/config.json`); `TJIKUP_CONFIG` or `--config` overrides it.
`--check` validates the entire schema and expands paths without running scripts;
it does not verify external applications or permissions. Missing configuration
fails with a useful error. `--help` works without a configuration.

## Configuration, version 1

```json
{
  "version": 1,
  "title": "My workflow",
  "env": {"WORKSPACE": "{config_dir}/project"},
  "flags": {
    "verbose": {"env": "MY_VERBOSE", "help": "enable script diagnostics"}
  },
  "steps": [
    {
      "id": "prepare",
      "label": "Prepare workspace",
      "verb": "BUILD",
      "command": ["bash", "{config_dir}/prepare.sh"],
      "preview": ["bash", "{config_dir}/prepare.sh", "--preview"],
      "cwd": "$WORKSPACE",
      "output": "stream",
      "timeout": 120
    },
    {
      "parallel": [
        {"id": "first", "command": ["bash", "{config_dir}/first.sh"]},
        {"id": "second", "command": ["bash", "{config_dir}/second.sh"]}
      ]
    }
  ]
}
```

Steps run in array order. A parallel group waits for every member, with at most
four workers, before proceeding. Any failure prevents subsequent stages.
`command` and `preview` are argv arrays: shell operators are never interpreted.
Use an explicit shell script when shell behavior is wanted. Configuration is
trusted executable policy, just like those scripts.

`{config_dir}` refers to the resolved configuration file's directory, including
through symlinks. `~/`, `$VAR` and `${VAR}` expand using the environment. Use
`$$` for a literal dollar sign. Undefined variables fail validation. `cwd`
defaults to the configuration directory; relative directories resolve there.
Top-level `env` applies to every command, and per-step `env` overrides it.
Boolean flags set their named environment variable to `1` or `0`; reserved
runner flags cannot be replaced.

`label` defaults to the unique step `id`; `verb` defaults to `RUN` and permits
1–7 uppercase letters. `timeout` defaults to 300 seconds. `output: "quiet"`
(default) hides successful stdout/stderr but prints diagnostics on failure;
quiet commands cannot read stdin. `output: "stream"` forwards stdout live and
keeps stdin/stderr available for interactive prompts. Parallel members must
use quiet output so prompts and status rows cannot interleave.

Exit zero means success. Optional `skip_codes` map specific nonzero codes to
SKIP and continue. Optional `stop_codes` map codes to a warning and stop the
remaining workflow with exit zero (for a recoverable condition requiring human
follow-up). Failures exit 1; interruption exits 130. A stop in a parallel group
does not cancel its already-running peers. `--dry-run` runs **only** explicit
`preview` commands and skips everything else; preview scripts are responsible
for not mutating state. No command is presumed safe by its name.

## Script reporting

Commands receive `TJIKUP_REPORT_FILE` (a temporary TSV report), `TJIKUP_COLUMNS`,
`TJIKUP_COLOR`, `TJIKUP_NO_SPIN` and `TJIKUP_DRY_RUN`. A script may append
`stage<TAB>icon<TAB>subject<TAB>note<NEWLINE>` records. Use `✓` for completion,
`!` for a warning, and `✗` for failure. Never put tabs/newlines or credentials
into those fields. The runner owns report creation, summary, one completion
notification and cleanup. Notifications include subjects, never raw command
output or notes. Set `TJIKUP_NOTIFY=0` to disable them. Direct Kitty SSH
sessions notify the originating desktop; detached/headless jobs stay silent.

Python scripts optionally import `tjikup.runtime` (the runner exposes its own
package through the child `PYTHONPATH`). `status_msg`, `stage_result`,
`stage_unchanged` and `stage_label` display and record rows. `run_stream` forwards
output with a deadline. This reporting API knows nothing about Git or any
configuration manager. Ordinary scripts need no Python integration.

The dotfiles repository owns its fleet workflow and capture scripts. Chezetc
owns system-apply behavior. Add another workflow by writing JSON and scripts;
no change to tjikup is required.

## Notifications over SSH

A direct Kitty SSH terminal receives completion notifications on the computer
running Kitty. Remote titles include the hostname. Use `ssh -t HOST tjikup`
for one-line commands; interactive SSH sessions already allocate a terminal.
The connection must remain open until completion. The local installation
provides the `tjikup-notification` icon. No remote `notify-send`, reverse SSH,
or desktop-bus forwarding is required. Redirected output and unsupported
terminals stay silent; tmux passthrough is not configured automatically.
`TJIKUP_NOTIFY=0` disables both local and SSH notification delivery.

## Grouping and script-owned status rows

An optional workflow `banner: {"title": "My project", "url": "https://example.org"}`
replaces the generic title with a magenta repository name and dim URL. A step or
parallel group may set `section: "Prerequisites"` and/or `banner` to introduce
its output. Parallel members cannot declare headings; put them on the group.
These are display-only fields and do not affect execution order.

Set a step's `summary: false` when its script already displays its own status
rows. This suppresses the runner's duplicate success/skip row (including the
preview skip); failures and stop warnings remain visible. Script report events
are still collected. The default is `true`.
