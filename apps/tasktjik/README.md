# Tasktjik

Standalone task app and authority for personal TODOs, reading items and agent work.
Run `tasktjik` for the Quickshell window. Runtjik embeds the same view through
`/todo` and `/reading`; Notjik sends actions through the Tasktjik CLI.

Build with `bun run build`; install with `bash scripts/install.sh tasktjik`.
Configuration is `$XDG_CONFIG_HOME/tasktjik/tasktjik.conf` (normally
`~/.config/tasktjik/tasktjik.conf`); see `examples/tasktjik.conf`.
Existing Runtjik Todo configuration is read as a fallback, with Tasktjik values
winning. The historical `~/.local/share/runtjik-todo` directory deliberately
remains the default: no records are moved, duplicated or rewritten on install.
Install the HTTPS authority on Si before updating clients. There is one writable
authority; unavailable authorities fail visibly. SSH endpoints must run the native `tasktjik-api serve` command. No Python
fallback is installed.

In the task list, press `P` to purge the selected task immediately. A three-second
Undo toast appears after deletion succeeds. `U` and Ctrl+Z remain available after
the toast disappears. Purge removes the entire task, including recurrence history.

`tasktjik request` accepts one JSON request on stdin and returns JSON on stdout.
`tasktjik state` returns UI state. `tasktjik serve` is the authority endpoint.
The desktop due-date field accepts `today`, `tomorrow`, `next Friday`,
`in two weeks`, `3 days from now`, `next month`, `15 December`, and ISO dates.
It previews the resolved date and saves `YYYY-MM-DD`; blank or `no due date`
clears it. Weekdays mean the upcoming occurrence (including today); `next`
excludes today. `next week` means seven days from today. Month/year offsets
clamp to the last valid day, and named dates without a year use the next
occurrence. Parsing uses the local calendar date when the editor opens.
Ambiguous numeric dates and unsupported phrases stay in the editor for correction.
Tab to an optional-field button such as `+ Due date` and press Enter (or Space)
to reveal and focus that field. As you type a due date, matching suggestions
show their resolved dates. Use Up/Down to choose and Enter or Tab to fill the
field without saving; Escape dismisses suggestions. Complete dates still save
with Enter. The editor keeps the title and link together,
with compact optional details and save actions below.

Task IDs, recurrence completions, revision checks and reading capture request IDs
retain their existing format. Agent creates should supply a stable UUID
`request_id` so retrying the same submission cannot duplicate a task.

```sh
printf '%s\n' '{"action":"create","request_id":"b2c65239-cf37-4182-9bd4-e642568b7f46","fields":{"title":"Verify task window","source":{"service":"agent","session":"example"},"assignee":"agent:example","project":"tjikos","acceptance":["Window opens and renders task state"]}}' | tasktjik request
```

Agent requests use `status` or `verify` with `id`, current `revision` and `actor`.
Status values are `inbox`, `ready`, `in_progress`, `blocked`, `done`, `cancelled`.
A blocked request must include `blockers: [{"reason":"Awaiting review"}]`;
blockers may reference an existing task with `task_id`.
Verification requires `outcome` (`passed`, `failed`, `pending`), `evidence`, and
`artifact` (a revision, build or other precise target). Records retain actor,
time, task revision and occurrence. Verification does not mark a task done.
Evidence remains attributed agent/user reporting, not independent attestation.
The UI displays status, assignee, acceptance, blockers and the latest evidence.
Creation does not launch agents or grant execution authority.

Enable `tasktjik.timer` on configured graphical clients for reminders. When
switching, disable `runtjik-todo.timer` first; the old Runtjik timer unit now invokes the same native Tasktjik CLI. Installation never enables or stops timers.
Keep all task records, configuration and evidence outside the repository.
See the HTTPS authority and backup instructions below.

## Private HTTPS authority

The authority is `tasktjik-api`, a Rust/Axum service with transactional SQLite
storage. Production clients configure `url = "https://si.tailb49776.ts.net/tasktjik"`
in `[tasktjik]`. `tasktjik connect URL --notifications` verifies the Tailscale
identity, registers this device for reminders and saves the endpoint. Omit
`--notifications` for devices that should only read/edit tasks.

Tailscale Serve provides HTTPS and identity headers over a protected Unix
socket. Root proxy peers must supply an explicitly allowed login; configured
local user IDs authenticate through kernel peer credentials instead of headers; tailnet membership alone does not grant task access. Browser clients
use the same API. The device list controls reminder delivery, not authentication;
remove a device from Tailscale to revoke its access. No app password is stored.

All mutations accept UUID request IDs for retry deduplication. Native clients
supply one automatically; integrations should retain the same ID when retrying
a request whose outcome is unknown. Updates reject stale revisions. Notification
claims have two-minute leases and are acknowledged after Notjik accepts them.
A crash between delivery and acknowledgement can still cause a later repeat;
this is coordinated delivery, not an exactly-once guarantee.

The database is `/var/lib/tasktjik/tasks.sqlite3` on Si. Synctjik backs it up with
online SQLite snapshots in the encrypted application-state repository; do not
sync live writable database copies. `tasktjik-api snapshot DATABASE NEW_FILE`
creates a verified private snapshot. `tasktjik-api import-json DATABASE DIRECTORY`
imports legacy JSON transactionally, retains originals and rejects conflicts.
Deployment policy, service units and enrollment activation are owned by Dotjik
and Tjikup. Task data, credentials and local device IDs stay outside Git.

The CLI, enrollment, device transport, reminder leases and notification IPC are
Rust, shared with the Runtjik adapter. Build and installation do not require
Python. The old source implementations have been removed; Python remains only
for executable and UI test drivers.

Build requires Cargo; `bun run test` in this app runs Python and Rust tests.

Si-local CLI clients use `socket = "/run/tasktjik/api.sock"`. This reaches the
same API and database without looping through the server’s nginx listener.
Socket access uses the tasktjik group, and UID 1000 maps to m@type.hk on Si.
The database directory remains mode 0700, private to the service user.

In the list, `/` or the search icon opens the text filter; Escape clears and
hides it. The sort menu switches between due date and title, in either direction.
Date badges use weekdays in weekly/today views and for this calendar week in
All tasks. Other dates use D/W/M/Y intervals; overdue open tasks use red labels.
The selected task shows a chevron in place of its date label.
Snoozed dates are underlined and drive filtering/sorting. Undated tasks sort last.

`U`, Ctrl+Z, or the undo icon reverses the latest reversible action made by this
device against the SQLite authority. Up to 100 actions are retained per device
and owner, including creation, edits, completion, snooze, status, verification,
and purge. Undo refuses to overwrite subsequent changes from another client or
notification worker. History starts when the updated authority is installed;
old actions and legacy flat-file storage do not have an undo journal.
Text fields retain their normal Ctrl+Z text undo.

Tasks can be dragged in every sort mode, within or between groups. The original
row stays at half opacity, a compact title preview follows the pointer, and a
gap opens at the destination.
Release to show the new group immediately while it saves; Escape cancels a drag.
The selected sort is applied immediately after each drop. **Manual** keeps the
dropped position; date and title sorting place the task in their sorted order.
Rejected moves restore the server state and show an error. Lists auto-scroll
near their edges, and empty groups remain available as destinations in every mode.
Manual positions are saved on the authority, synchronize across
clients, and can be undone with U/Ctrl+Z. Revision checks protect newer edits.
