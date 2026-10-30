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
Install Tasktjik on Si before updating clients. There is still one writable
authority, reached through bounded SSH; unavailable authorities fail visibly.

`tasktjik request` accepts one JSON request on stdin and returns JSON on stdout.
`tasktjik state` returns UI state. `tasktjik serve` is the authority endpoint.
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
switching, disable `runtjik-todo.timer` first; the compatibility helper remains
available for older integrations. Installation never enables or stops timers.
Keep all task records, configuration and evidence outside the repository.
Back up the existing authority directory using the existing Synctjik policy;
this extraction makes no claim of deployment or verified backup publication.
