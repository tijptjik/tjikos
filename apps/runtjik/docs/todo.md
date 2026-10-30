# Tasktjik integration

Task storage, scheduling and the shared view now belong to
[Tasktjik](../../tasktjik/README.md). Runtjik embeds the view as `/todo` and
`/reading`. Install Tasktjik on the authority before updating clients.
The details below describe the retained task behavior; new configuration uses
`tasktjik/tasktjik.conf` and new reminder installations use `tasktjik.timer`.


`/todo` is an app provider: the launcher search bar collapses and typing invokes
commands. `/` opens task filtering; Escape exits filtering, then exits the app.
The two-column list groups tasks by domain. The default view includes overdue
and this week's tasks; Shift+N shows today, `l` includes every date and undated
tasks, and `w` returns to this week. Completed tasks remain visible with a strike
through, including tasks completed today outside the date filter.

`n` creates a task; `d` toggles the selected occurrence; `t` opens title editing;
`g` focuses the group dropdown; `s` opens snooze. Up/Down or j/k select tasks.
The requested `n` bindings conflicted, so today uses Shift+N.

The editor includes title, group, tags, ISO due date, repeat interval and plan.
Quick always sets due to today. Repeat unit is days, weeks, months, years,
days-of-the-week (ISO Monday=1 through Sunday=7), or days-of-the-month (1–31).
Value is a positive interval or comma-separated day numbers. Month/year intervals
clamp to month end without drifting from the original anchor. Completion history
is keyed by occurrence date; toggling one occurrence preserves the others.
The task owner is `tijptjik`; notification lead defaults to one day, with hours,
days and weeks supported by the storage API. Each edit checks a revision number.

## Authority and setup

Todo data lives outside Dotjik and this repository. Each task is a private JSON
file containing fields, completion history and notification claims. Writes use an
exclusive authority lock, atomic replacement and fsync. There is one writable
store: **Si**. Clients use bounded, noninteractive SSH calls to its installed
`~/.local/lib/tasktjik/tasktjik.py serve`. No credentials are stored in Todo settings.
When Si cannot be reached, changes fail visibly; there is no offline writable copy.

Install the built Runtjik helper on Si and updated Runtjik and Notjik on clients.
Add `Todo` to an existing `plugins.enabled` list (fresh defaults include it).
Configure each client's existing runtjik.conf:

```toml
[plugin_settings.Todo]
authority = "si" # configured SSH host, with an already trusted host key
# This path is on Si, not the client:
directory = "~/.local/share/runtjik-todo"
# This path is local to each client:
vault = "~/path/to/Obsidian"
```

An empty authority enables an explicitly local store for standalone use. Demo
mode always uses temporary synthetic state and never calls Si or scans a vault.
Plan lookup uses fuzzy subsequence matching over Markdown filenames. The editor
stores a vault-relative filename, not the file contents.

Install supplies `runtjik-todo.service` and `runtjik-todo.timer`, without enabling
them. On each graphical client, enable the timer after configuring the authority:

```sh
systemctl --user daemon-reload
systemctl --user enable --now runtjik-todo.timer
```

Every five minutes a client checks due tasks and atomically claims one reminder
per task occurrence per authority day. Only the winning device shows a dedicated
Notjik card. Done, Dismiss and Snooze wait for successful persistence before
closing. Snooze presets include afternoon, evening, night, tomorrow, weekend,
next week and a Monday-first calendar with distinct weekend colors; chosen dates
use 09:00 local time. Snoozing permits a reminder after the snooze expires.

Claims favor avoiding duplicate delivery: if a client crashes after claiming but
before displaying, that day's reminder can be missed. Notification transport and
remote storage cannot provide exactly-once delivery together. Dismiss suppresses
the current day's reminder; Done completes its captured occurrence. Claims use
Si's calendar day; clients should use the same timezone. There is no live removal
of a card already displayed on another device. Its Done action remains idempotent.

## Encrypted backups through Synctjik

Use Synctjik for recovery, not for merging live writable copies. Enroll this
collection on Si in the existing v2 archive policy's `folders` object:

```json
{
  ".local/share/runtjik-todo": {
    "mode": "device",
    "devices": ["si"],
    "encrypted": true,
    "retention": "daily-weekly-monthly",
    "include_files": ["*.json"]
  }
}
```

`device` keeps recoveries separate from a running authority. Existing daily
Dotfiles LFS scheduling then archives the enrolled collection via tar/zstd + age.
The source JSON files must never be added to Dotjik. The policy entry itself is
non-secret configuration. Before declaring enrollment complete, run the policy
check, publish a verified archive on Si, and restore JSON files into a new private
directory. Restore the authority only during an explicit maintenance window with
clients stopped. This change supplies the policy fragment; it does not change
fleet policy, deploy Si, enable timers, or claim a verified backup receipt.

## App provider interface

Providers may declare `mode = 'app'` and implement `state(request, settings)`.
Command query responses carry `app_mode` and `app_state`; `app-action` requests
are routed only to enabled app providers. Todo supplies the first dedicated QML
view. Search providers retain their existing query and activation contracts.
