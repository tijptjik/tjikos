# Notjik notification service

Private global history and active-device delivery for Notjik. Si runs Rust/Axum
with transactional SQLite, using the same protected Unix socket and Tailscale
Serve identity architecture as Tasktjik. The services have separate databases and
lifecycles. The endpoint is `https://si.tailb49776.ts.net/notjik`.

Desktop notifications remain local by default. A producer independently selects
`delivery: local|active` and `history: false|true`. Tjikup results, scheduled runs,
Synctjik storage/backup results and Tasktjik reminders use active delivery and
retained history after enrollment. Ordinary Telegram chat handling stays local.
The client's `history_exclude` defaults to Telegram, Telegram via Notjik and
org.telegram.desktop; the Telegram entry also covers Telegram Desktop. Excluded
envelopes can be routed but are absent from global
history. Settled non-retained transport envelopes expire after one day.

## Presence and delivery

An unlocked, unpaused desktop with input in the last 120 seconds is eligible.
Heartbeats expire after 35 seconds. The most recently active eligible device wins;
opening Pi's drawer also makes Pi active. Background phone use does not compete
with a desktop. Pi is the fallback when no foreground client qualifies, provided
its notification permission, push registration and per-device DND allow delivery.
With no eligible device, notifications remain pending. There is no critical-alert
DND bypass.

Delivery uses 45-second leases and receiver acknowledgement. Stable notification
IDs and device receipts suppress replay; retries are at-least-once, not an
exactly-once guarantee across a crash between display and receipt persistence.
A local SQLite outbox survives producer or server restarts. If Si cannot be reached,
a desktop can display locally and synchronize the result as already shown. An
ambiguous network failure can still race a server-side delivery on another device.
Headless producers keep the pending result durably without requiring a desktop.

History retains managed results indefinitely. A sender's `(device, app, run)`
identity updates its card; separate run IDs preserve separate results. Both drawers
search global history by app, device, run and content, with paginated older results.
Desktop offline history uses the latest cached page. Dismissal is global for shared
cards; read state is separate. DND remains per device. Historical local entries are
not uploaded retroactively.

## Cross-device actions

Commands are registered in the originating device's private SQLite database. Only
an opaque action UUID and label cross the network; the authority never accepts
remote command arguments. Another device can invoke the registered action and
inspect its queued, started, succeeded, failed or unknown result. HTTPS links open
locally. Receiving devices never execute a sender's command locally.

Execution is claimed once, recorded before launch, and limited to five minutes.
A crash or ambiguous outcome is never automatically retried. Started jobs without
a conclusive result become unknown after ten minutes; a later definitive outcome
can resolve that state. Repeated taps while a job is pending/started share the job.
A deliberate invocation after completion creates another execution.

## Build and enroll

```sh
bun install
bun run --filter @tjikos/notjik-service build
notjik-sync connect https://si.tailb49776.ts.net/notjik fi desktop
notjik-sync devices
notjik-sync run
```

Private enrollment is stored at `~/.config/notjik/sync.json` (0600), state at
`~/.local/state/notjik/sync`. `NOTJIK_SYNC_CONFIG` and `NOTJIK_SYNC_STATE` select
isolated locations. Each device has a separate credential, bound to the allowed
Tailscale login. Si-local clients can pass `/run/notjik/api.sock` as a fourth
connect argument with `http://localhost`. Root producers use
`/etc/notjik/system-client.json` and `/var/lib/notjik-system-client`.

```sh
notjikctl send 'Build complete' 'All checks passed' --app build \
  --id run-123 --delivery active --history global \
  --actions '[{"label":"Inspect on source","command":["xdg-open","https://example.com/build/123"]}]'
notjik-sync history 'build'
notjik-sync history '' 100
notjik-sync jobs
```

`notjik-sync publish` accepts a notice JSON object on stdin for headless scripts.
`notjik-sync invoke ID ACTION`, `read ID`, and `dismiss ID` queue durable mutations.
`flush` retries the outbox. `request` is the low-level JSON protocol client.
All protocol mutations use UUID request IDs and reject conflicting reuse.

Dotjik owns system units, enrollment activation and backup policy. Its scoped
`scripts/install-notjik-service.sh` and `scripts/activate-notjik-api.sh` install
this service without touching pinned archive workers or restarting Tasktjik.
Tailscale Funnel must remain disabled. Si's root proxy supplies authenticated
identity headers; untrusted socket peers cannot impersonate that proxy.

## Android and Firebase

The native Kotlin/Compose application is in `../notjik-android`. It can be sideloaded
and later distributed through Play with the same application ID/signing lineage.
FCM is free and works with sideloaded apps on Google Play services devices. It is
a wakeup transport: its payload contains only an opaque `reference`. The app fetches
all content from Si over Tailscale, then posts the Android notification. There is
no sensitive-content fallback through Firebase. Android permissions, Doze, network
availability and OEM background policies can delay delivery; WorkManager retries
and periodic reconciliation recover missed wakeups.

Place the Firebase service account JSON at the private path configured by
`server.firebase_credentials`, owned by notjik and mode 0600. The account needs
FCM message-send permission for the selected project. `server.android_config`
points to a JSON object with public Firebase client identifiers:
`app_id`, `api_key`, `sender_id`, `project_id`. Register Android package
`hk.type.notjik`. The authenticated Android enrollment fetches these values from
Si; no Firebase or account configuration belongs in Git.

## Verification and recovery

```sh
cargo test --manifest-path packages/notjik-sync/Cargo.toml --locked
cargo test --manifest-path apps/notjik-service/Cargo.toml --locked
python apps/notjik-service/tests/delivery.py
```

The tests cover routing/DND, phone fallback, deduplication, action ownership,
authentication, cross-origin rejection, server restarts, offline fallback and
history exclusions. Integration fixtures use a private temporary authority and
synthetic content only.

The authority database is `/var/lib/notjik/notifications.sqlite3`. Synctjik's
application-state policy includes it and the system producer outbox, using online
SQLite snapshots in the encrypted Restic repository. Never copy a live writable
SQLite file without its snapshot API. `notjik-api snapshot DATABASE NEW_FILE`
creates and integrity-checks a consistent snapshot. Restore into a separate
location and verify integrity before stopping only notjik-api and replacing its
state. Device enrollment, outboxes, action registrations and Android signing keys
are private runtime data and must be preserved separately from source code.
