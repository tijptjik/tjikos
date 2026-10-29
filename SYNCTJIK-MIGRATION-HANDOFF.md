# Synctjik backup-engine migration: separate LLM workstream

The user authorized this work separately from the Tjikup migration. Own backup,
restore, replication and backup-health behavior in Synctjik. Tjikup owns fleet
orchestration, template/file deployment, installers and ordinary maintenance.

## Scope

Port the remaining Python Google Drive/Restic backup, Si application snapshot,
pull and restore, and SnapRAID maintenance/notification engines to Rust under
`apps/synctjik`. Update the matching policy, wrappers, recipes and systemd units
in `/home/io/.local/share/dotjik` to invoke Synctjik. Keep application runtimes
and development test tools separate from fleet-helper dependencies.

Preserve storage layouts, backup formats, locks shared with app updates, durable
receipts, freshness checks, database consistency, retention, health semantics,
mount/UUID guards, restore preflights and failure recovery. Keep downloaded or
source data until successful verification. Never turn a running worker or a
successful service result into a claim of recoverability without evidence.

## Coordination and boundaries

- A concurrent Tjikup migration owns public configuration/Git/SSH commands,
  installers and ordinary Si maintenance (storage, permissions, media guard,
  Transmission cleanup, NSS and app updates). Preserve those changes.
- Tjikup's ordinary maintenance policy is `/etc/tjikup/maintenance.json`;
  systemd invokes `/usr/local/lib/tjikup/tjikup system ...`.
- Replace the old restore engine's `si-provision/storage.py` import with native
  storage checks. Remove that compatibility module when no consumer remains.
- Publication and release-pin updates are explicitly deferred. Do not publish,
  tag, push, commit unrelated work or advance bootstrap pins.
- Shared checkouts are dirty and other actors may commit. Inspect before editing;
  preserve unrelated files, staged changes and concurrent updates.
- Do not interrupt, restart or replace pinned running archive workers. Do not
  run live backups, destructive restores, broad configuration applies or remote
  maintenance as validation.
- Keep mail, tokens, credentials, private account/configuration data and captures
  outside either repository. Use synthetic fixtures only.

## Acceptance

Provide native commands with useful help and equivalent exit/status behavior.
Exercise successful and failed backups, stale/missing receipts, mount mismatch,
lock contention, interrupted publication, retention and representative restore
with temporary storage and fake external commands. Port meaningful existing
Python tests; retained test oracles must not be deployed runtime dependencies.
Update operational docs and callers, retire obsolete managed executables
explicitly, and report source/test completion separately from live acceptance
and release availability. Include remaining limitations and exact validation.
