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

Reading has list and grid views for saved web pages, with titles, domains,
page images, descriptions and an Open page action. It defaults to an ungrouped
list sorted by recently saved; the category toggle replaces the cancelled-task
filter and shows only categories containing matching pages. Completed pages are
labelled Read and can be included with the read-pages toggle. Enter opens the
selected page; D marks it read or unread.

Previews render pages using installed Chrome or Chromium in headless mode with
a fresh, temporary profile; they never use your browser cookies or a third-party
preview service. Python 3 fetches descriptions and fallback site images. Browser
error pages and sign-in screens are rejected; sites that block rendering show a
favicon fallback when available, otherwise a clear placeholder. Successful images
and their metadata persist across restarts under
`$XDG_CACHE_HOME/tasktjik/page-previews` (normally `~/.cache/tasktjik/page-previews`).
Cached images load before any missing page is rendered and remain available until
the cache is cleared; missing image files are regenerated on the next load.
Pages without an available image retain a domain placeholder and their link.
Previews with no usable image retry after five minutes on the next load; legacy
cache entries are refreshed automatically. Rendering requires Chrome or Chromium;
without it, site-provided images remain available.
Run `python3 -B tests/page_preview.py` for the cache regression checks.
Run `python3 -B tests/reading_ui.py` for the isolated synthetic renderer check.

Select a task in the desktop overview and press **Ctrl+V** to attach an image
from the Wayland clipboard (`wl-paste` is required). PNG, JPEG and WebP are
supported, up to 5 MiB per image and 20 images per task. Repeatedly pasting the
same image does not add another copy. Text fields keep their normal paste behavior.
A small image icon follows the task name. Selecting a task with images opens a
scrollable image details pane in the half opposite its column; arrow navigation
still crosses columns and moves the pane. A one-column overview shares its width
with the image pane on the right.

Install the updated authority as well as the client before using attachments.
Image bytes live in the authority's SQLite database, separately from task-list
metadata, and are included in verified snapshots. Attaching images is undoable;
bytes are retained so undoing a purge can restore them. Legacy local/SSH storage
uses a private `images/` directory alongside task JSON; include both when copying
that storage (`import-json` imports the images too). No image is stored in Git.
Run `python3 -B tests/images_ui.py` for the isolated synthetic clipboard and
renderer check.

In the task list, press `P` to remove the selected task from the list immediately
while deletion is sent to the authority. A rejected deletion restores the row
and its selection. A three-second Undo toast appears immediately. Undo restores
the row immediately too: it cancels an unsent deletion, or queues a restore after
the in-flight deletion completes. Rapid deletions and restores run in order;
failed restores roll back dependent queued restores and retain their retry order.
`U` and Ctrl+Z remain available after the toast disappears. Purge removes the
entire task, including recurrence history. The UI queue is held in memory.

`tasktjik request` accepts one JSON request on stdin and returns JSON on stdout.
`tasktjik state` returns UI state. `tasktjik serve` is the authority endpoint.
The standalone window uses `state --incremental`: mutations return just the
acknowledged task or deleted ID, which is merged into the displayed list. Regular
polls fetch the full list to reconcile other devices. A mutation does not wait
for an additional full-list request before the UI confirms it.
Enter or Save returns to the list immediately and shows the task as **Saving…**
until the authority confirms it. Repeated submissions are ignored while that
save is pending. A failed save restores the draft; retrying it unchanged keeps
the same request ID, so a lost reply cannot create a second task. Pending saves
are held in memory, not a durable offline queue.
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

Tasks also have a `metadata` JSON object for programmatic identification and
enrichment. It defaults to `{}` for new and older tasks. Set it through
`fields.metadata` on `create` or revision-checked `edit` requests, for example
`{"action":"create","fields":{"title":"Follow up","metadata":{"external_id":"guest-42","context":{"event":"example"},"contacted":false}}}`
passed to `tasktjik request`. Values may be any JSON type, including nested
objects and arrays; the top-level value must be an object. Supplying metadata
replaces the whole object, omitting it preserves it, and `{}` clears it.
The native and web task edit footers show sorted `key : value` pairs in monospace,
flowing horizontally and wrapping as needed. Strings display without quotes,
empty strings display as `-`, and other values use JSON. Metadata is read-only and is
never included in their save payloads. Update the authority as well as clients
before using this field; supported authorities advertise `metadata: true`.

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
Closing the desktop window keeps its shell available; launching Tasktjik again
reopens that window and preserves its navigation state. Run
`python3 -B tests/window_reopen.py` for the isolated close/reopen regression check.
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

## Managed reminder delivery

With Notjik sync enrolled, reminder claims publish durable global cards instead of
requiring a local graphical session. Si runs the reminder timer so Pi can receive
reminders while both desktops are offline. `tasktjik enroll --notifications`
registers the current device using its existing HTTPS or Unix-socket transport.
Desktop and Android cards can mark a task done or dismiss its reminder through
registered actions on the originating device. Tasktjik remains the task authority;
Notjik owns notification routing/history.

## Browser reading capture

In Runtjik, Shift+Enter or Shift+click on a browser tab opens its title and URL
in the reading editor. **Save and close tab** creates a reading item through the
configured HTTPS authority on Si (or its local HTTP Unix socket), then asks the
browser bridge to close that exact tab and URL. Cancel and failed saves leave
the tab open. If closing fails after a successful save, the item remains saved
and the UI reports the close failure.

Retries of a submitted draft use the same UUID and fields, including after a
lost HTTP reply. Confirm the original submission before editing the saved item.
Capture drafts are held in the launcher process; after restarting it, check
Reading before capturing a tab whose save outcome was unknown. Browser session
identifiers stay on the client. Only HTTP and HTTPS references are accepted.

## Projects, templates and subtask flows

The **Projects** tab lists projects, and **New** there creates a project.
Open **Settings → Templates** to create or edit a template; task views create tasks. A project appears as one
card with a completion bar. Open it (or press Enter) for the usual
grouped task view scoped to its members. Drag its card between groups to change
its grouping. In the project editor, **Only show Project tasks in their project**
is enabled by default; disable it to also display members in the index. Moving a
project changes its own grouping, preserving members' groupings inside it.
Projects with members cannot be deleted until those members are reassigned.
**Settings → Project sorting** offers **Always First**, **Always Last**, and
**First Due** (the default). First/last placement applies within each group for
every task sort order. First Due uses the earliest effective due date among
unfinished, non-cancelled members, including hidden members; projects without
dated members follow dated tasks when sorting by due date. The choice is saved
locally and shared with the Runtjik task view.
Empty groups are shown only during a drag. Double-click a task, use the pencil,
or press **E** to edit it. Hover a task to reveal its animated completion checkbox;
**D** also toggles completion. **C** immediately cancels the focused task, with
rollback if saving fails. **Cancel task** and **C** also work in the editor
(C does not intercept typing in text fields). Click a project or press
**Enter** to open it, with its groups starting in column one.

Double-click a group heading to edit its name in place for only the tasks currently
visible. Enter or Save submits; Escape or Cancel discards the draft. A failed save
keeps the field and error visible for correction.
Settings opens a separate window with searchable General, Groups, and Templates
sections. Its transient parent is the invoking task window; Dotjik's Hyprland rule
floats it and centers it over that window.
Task groups retain their delegates across column changes, and filtered/sorted
rows are cached independently of window geometry. Run
`python3 -B tests/resize_benchmark.py` for a synthetic 112-task resize workload;
add `--wayland` for an isolated compositor with hardware rendering and per-client
GPU engine time when the driver exposes it. These measurements do not include
the desktop compositor's GPU work.
**Settings → Groups** manages saved groups, including empty ones. Add, rename or
remove a group there; removing moves its tasks to Inbox. Drag a group's handle
into another group to merge, then confirm in the modal. Global group changes
check all affected revisions and membership, preserve deadlines, and commit as
one undoable authority transaction. Local JSON storage uses a roll-forward
journal to recover interrupted batches. API actions are `group-add`,
`group-rename`, `group-merge`, and `group-remove`, with `groupsRevision`, `source`,
`target`, and `members: [{id, revision}]`; `scoped: true` limits a rename to those
members. Rendered state includes `groupsRevision`.

Progress pips indicate subtasks: click the card or pips, or press **Enter**, to
expand or collapse them. Each primary step or flow contributes a pip, and active
branch steps add their own. A flow completes after its selected branch completes.
Click a subtask row or press **Enter** while it is focused to toggle completion.
Rows use dots that reveal a checkbox on hover; red means exit and yellow means
reminder. The date column uses the same compact codes as the main task. The hover **+** adds the first
subtask; an expanded list has a **+** at the end. The editor supports
ordinary steps and flow blocks. Select a flow to add alternatives and set its
shared deadline. An alternative can be an ordinary choice, an **Exit**, or a
**Reminder**. Select an ordinary alternative to add branch subtasks.

Completing a choice cancels its siblings and activates its branches. Completing
an exit also cancels subsequent steps. Reopening the choice restores the previous
completion history and hides its branches again. Reminders accept intervals such
as `one week before`, `2 days before the deadline`, or `on deadline`; they remain
snoozed until that date. Completing a reminder leaves the choice open. Another
choice cancels unfinished reminders but retains completed ones. Due reminders
surface their owning task in date views and use the existing notification leases;
notification completion targets the reminder, not the entire task.

The **Workflow** toggle in the template/task editor makes each primary step wait
for the preceding step. Alternatives are mutually exclusive choices, not a chain;
activated branches must finish before the next primary step unlocks. Blocked steps
cannot be completed or send reminders. Reopening an earlier step blocks later steps
without erasing their completion history. **Show blocked** (the lock icon) reveals
blocked tasks and steps; they are hidden by default. API clients set `fields.workflow`
to a boolean, copied from a template when creating from it. Rendered stages include
`dependsOn` and `status`, and `subtaskPips` carries the compact progress indicators.

**Templates** are editable reusable flows. Select **From Template** on a new task
to make an independent copy, with all completion flags reset. Later template edits
do not modify existing tasks. Main-task completion remains an explicit action.

API clients create records with `fields.kind` set to `todo`, `project`, or
`template`, link tasks using `fields.project` (the project UUID), and instantiate
with a top-level `template_id` on `create`. `fields.subtasks` is a list of nodes:
`{id,title,kind,due,done,items}`. Flow nodes contain choices in `items`; choice
nodes contain branch steps. Reminder nodes also have an `offset` string. Normal
steps and flow blocks own their due dates; choice dates must be empty. Updates
use `subtask-toggle` with `id`, `revision`, and `subtask_id`. As with other
mutations, retain the UUID `request_id` on retry. A whole flow change is one
revision-checked, undoable transaction. Rendered state includes `workflowRows`
and `subtaskProgress`; raw records retain explicit completion flags only.

Validation: `bun run test` covers authority/model/CLI behavior;
`python3 -B tests/projects_ui.py` exercises the native renderer with synthetic
projects, template copies, flow choices, exit reopening, branches and dragging.

Repeat schedules accept phrases such as `every two weeks`, `every Monday and
Friday`, and `on the 1st and 15th of every month`. Enter or leave the field to see
the interpreted schedule; the pencil lets you retype it. Unsupported phrases stay
editable with an error rather than being guessed.

A repeat no longer requires a due date. Without a deadline, the authority stores
`repeat.start` when the schedule is created. Clearing a legacy recurring task's
deadline preserves its previous anchor and completion history. Calendar-month
and yearly schedules retain month-end/leap-year behavior; weekdays and day-of-
month schedules use their next matching calendar day. Scheduled occurrences can
still become due and send reminders. Today, This week, and This month also include ordinary
undated tasks, while respecting future snoozes and project visibility.

`python3 -B tests/subtask_ui.py` checks workflow gating, compact controls, date
alignment, the month view and vertically centered empty states with synthetic data.

Completion controls update immediately, including workflow dependencies, branches,
reminders and progress pips. Requests are serialized with revision guards; rapid
changes can queue without waiting for the previous response. Rejected saves roll
back that task and its queued dependent changes while independent tasks continue.
Background refreshes cannot replace a pending optimistic projection.

Up/Down traverse expanded subtasks, skipping flow headers and inactive options,
then continue to the next task. The focused subtask survives save reconciliation.
Hover help and add controls use the app theme. `tests/optimistic_ui.py` checks
latency, failure rollback, navigation and control geometry with synthetic data;
`tests/workflow_projection.test.ts` compares the shared client projection against
the Rust authority.

Mouse-wheel scrolling animates the full 160-pixel step without an initial jump.
Rapid turns gradually increase travel up to twofold;
reversing direction, clicking, or dragging the scrollbar stops the previous glide.
Wheel scrolling stops at either edge without overshooting or restarting an outward
glide. Touchpad pixel scrolling keeps Qt's native behavior.

Conditional pathways can set `requires: [stepId, ...]` on a step or flow to wait
for those earlier steps instead of the normal sequence. A flow dependency waits
for its selected branch to finish. Cancellation propagates only to dependent
steps; `acceptCancelled: true` makes a join wait until every dependency is done
or cancelled. A `group` node contains independent steps and resolves automatically.
Reopening a dependency blocks its successors without erasing completion history.
The desktop and web editors expose dependency selection and cancellation. On the
desktop, **C** cancels the focused ordinary step; **C** again restores an explicitly
cancelled step. API clients use revision-checked `subtask-cancel` with `subtask_id`.
Template copies remap dependencies and reset completion/cancellation. Install the
authority and clients together; capability `conditional_pathways` indicates support.
