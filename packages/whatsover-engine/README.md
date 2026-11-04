# Whatsover Rust engine

Shared `oxidezap/whatsapp-rust` engine for a direct desktop CLI and the
`apps/whatsover-cloudflare` Durable Object service. The SDK is pinned by Git
revision in Cargo.toml and dependencies are locked in Cargo.lock.

The public desktop command is `whatsover`, backed by Rust. The former Baileys
launcher and service setup are retired. Its private account data is kept separate
and is not imported into the Rust session.

## Desktop

Requires Rust 1.99+ and a native C compiler for bundled SQLite. From the repo root:

```sh
bash packages/whatsover-engine/scripts/install.sh
whatsover login
whatsover status
whatsover check
whatsover send --to +15555550100 --text 'Hello' --request-id example-1
whatsover inbox --after 0 --limit 100
whatsover result example-1
```

The phone number above is synthetic; replace it with your intended recipient.
The installer builds a standalone native binary into `~/.local/bin` (override
with `WHATSOVER_INSTALL_PREFIX`). It replaces the old Baileys launcher and removes
the former `whatsover-rust` binary; it does not start a service. The checkout
command is `bun run whatsover`.

`login` renders a pairing QR on stderr; scan it with WhatsApp → Linked devices →
Link a device. It exits after connecting and flushing the session. `status` shows
only whether local paired credentials exist; `check` connects to verify they
still work, sends no chat message, and disconnects. `run` pairs if necessary and
keeps receiving until Ctrl+C. After stopping it, `inbox` reads locally persisted
messages without a network connection.
New desktop pairings identify themselves as **Whatsover** in Linked devices.
WhatsApp receives that name during initial pairing, so updating the binary does
not rename an already linked device registered as “Rust”.
All CLI result data is JSON on stdout. `send` connects directly to WhatsApp and
shuts down after the attempt. `send` and `check` fail with a login instruction if
unpaired; they never display pairing credentials in unattended job logs. Nothing
is routed through Cloudflare.

State defaults to `$XDG_STATE_HOME/whatsover-rust` or
`$HOME/.local/state/whatsover-rust`. Override with `WHATSOVER_RUST_STATE_DIR` or
`--state-dir /private/directory` (the flag takes precedence). These state paths
and the environment variable retain their original names to preserve existing
Rust logins and send ledgers when upgrading the command name.
The directory must have mode 0700 and live outside any Git repository. An
exclusive account lock prevents two CLI processes from owning the same session.

## Unattended scripts

After the one-time login, use the installed command from shell, Python subprocess,
cron, or a systemd user timer. No Bun, Cargo, browser, or background receiver is
needed at runtime. Use `--stdin` to keep message text out of process arguments:

```sh
printf '%s' 'The scheduled job finished.' | "$HOME/.local/bin/whatsover" send \
  --to '+15555550100' --stdin --id 'daily-job-2026-11-03' --timeout-seconds 60
```

Choose one stable ID per intended message, and retain it across retries. `--id`
is an alias for `--request-id`. Direct recipients need international numbers;
groups use their full `…@g.us` JID, not a display name.

Use `send --image /private/example.jpg --to ... --id ...` to send a JPEG, PNG, or
WebP image up to 8 MiB as a separate message. Optional `--text` or `--stdin`
supplies its caption. Image bytes participate in request-ID identity: changing
the file while reusing an ID is rejected. Uploaded media descriptors and keys
stay in the private session database.

For desktop `send`, exit 0 requires a matching, successful WhatsApp server
acknowledgement. The client waits for startup sync before sending and stays
connected for that acknowledgement. `--wait-delivery` additionally requires a
matching recipient delivery/read receipt; it does not count own-device sync or
read-self receipts. Offline recipients can therefore cause a delivery timeout
even when the server has accepted the message.

`result ID` returns the stored JSON plus `transport` evidence (or `null` if
absent) without sending. Old `submitted` records without acknowledgement
evidence are unconfirmed and cannot produce a successful replay. Replays never
resend. Rejection, uncertainty, validation failure, lock conflict, interruption,
or timeout exits nonzero. A timed-out send may already have reached WhatsApp:
inspect its result and the chat; do not generate a new ID automatically. The
default operation deadline is 180 seconds; shutdown drains separately afterward.

`whatsover observe --seconds 30` connects to collect outstanding server and
delivery receipts without initiating a new chat send. Evidence is persisted in
the private account database and included in subsequent `result ID` output.
It cannot reconstruct receipts that an older client already consumed and lost.

Only one process can use a Rust account at once, including offline commands.
Stop `run` before sending and serialize scheduled jobs. For example, wrap the
send command with `flock -w 240 "$HOME/.local/state/whatsover-rust/jobs.lock"`
after login has created the private directory. All jobs must use that same lock.
No schedule or recipient is configured by the installer.

## Contact names

`whatsover contact --stdin` accepts a JSON object or an array of up to 100 objects
with `to`, `firstName`, and `lastName`. It validates the whole batch before
connecting, then saves each name through WhatsApp contact sync and requests
primary-phone address-book sync. It does not send a chat message. A `saved` JSON
line records each server-accepted contact write; it is not independent proof that
the phone has applied the update. A failed batch stops at the first error, leaving
earlier acknowledged saves intact. Reapplying the same names is idempotent.

```sh
printf '%s' '{"to":"+15555550100","firstName":"Test","lastName":"Person"}' |
  whatsover contact --stdin
```

## Delivery contract

- Native auth, Signal state, inbox, and outbox share the SDK SQLite connection
  pool, using synchronous FULL commits.
- Encrypted inbound direct/group messages are saved by the SDK's durability hook
  before acknowledgement. Deduplication uses `(chat, sender, message ID)`.
- A send reserves its request ID durably before attempting network I/O. Reusing
  the ID with the same input returns the saved result; different input is rejected.
- `submitted` only means the SDK send call completed. On desktop, inspect
  `transport.server` (`accepted`, `rejected`, or `unconfirmed`) and
  `transport.recipientDeliveryConfirmed` separately. Neither server acceptance
  nor recipient delivery means a person read the message.
- Interrupted attempts become `uncertain` on restart. They are not automatically
  resent. Inspect the recipient conversation before choosing a new request ID.
- Channel/newsletter messages and historical archive import have separate SDK
  capture paths and are not currently part of this inbox contract.

## Verification

```sh
cargo test --manifest-path packages/whatsover-engine/Cargo.toml --locked
```

The Cloudflare workspace tests additionally run the compiled Rust backend inside
workerd, exercising the actual JS/SQLite boundary, conditional buffer operations,
state merges, full SDK startup, and Durable Object eviction recovery.
