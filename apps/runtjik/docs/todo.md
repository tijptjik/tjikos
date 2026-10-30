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

The editor includes title, group, tags, ISO due date, repeat interval and reference. Groups are free text with existing-group suggestions,
defaulting to All. Existing Plan values remain available as references.
Quick always sets due to today. Repeat unit is days, weeks, months, years,
days-of-the-week (ISO Monday=1 through Sunday=7), or days-of-the-month (1–31).
Value is a positive interval or comma-separated day numbers. Month/year intervals
clamp to month end without drifting from the original anchor. Completion history
is keyed by occurrence date; toggling one occurrence preserves the others.
The task owner is `tijptjik`; notification lead defaults to one day, with hours,
days and weeks supported by the storage API. Each edit checks a revision number.

## Authority and setup

Tasktjik owns persistence and reminders. Production clients use its Rust HTTPS
or Unix-socket API and transactional SQLite authority. Configure the endpoint in
`$XDG_CONFIG_HOME/tasktjik/tasktjik.conf`; Tasktjik settings take precedence over
Runtjik's embedded Todo settings. See [Tasktjik setup and backup instructions](../../tasktjik/README.md).

Add `Todo` to an existing `plugins.enabled` list (fresh defaults include it).
An optional local Markdown vault remains a Runtjik view setting:

```toml
[plugin_settings.Todo]
vault = "~/path/to/Obsidian"
```

Demo mode uses synthetic state. Plan lookup searches Markdown filenames and
stores a vault-relative filename, never the file contents. Use `tasktjik.timer`
for reminders on enrolled graphical clients. Installation does not activate timers.
Task records, credentials, device identities and backup snapshots stay outside Git.

## App provider interface

Providers may declare `mode = 'app'` and implement `state(request, settings)`.
Command query responses carry `app_mode` and `app_state`; `app-action` requests
are routed only to enabled app providers. Todo supplies the first dedicated QML
view. Search providers retain their existing query and activation contracts.

## Reading list

`/reading` opens reading items; the header selector switches between Todo and
Reading. Reading items inherit task scheduling, tags, reminders and completion.
Reference contains the HTTP(S) URL; Enter opens it without marking it done.
The browser plugin's Shift+Enter opens a prefilled filing panel and closes the
tab only after the authority acknowledges durable creation. Cancellation and
save failures leave the tab open. Saved items are retained if tab closure fails.
The authority must be upgraded alongside clients for reading support.
