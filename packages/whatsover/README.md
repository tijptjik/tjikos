# Whatsover

A small Bun/TypeScript library and CLI for sending WhatsApp text messages using
your own linked account. Baileys runs in one service; scripts share that service
over a private Unix socket, or an authenticated HTTP API. No browser is required.

## Start locally

From the repository root:

```sh
bun install
bun run build --filter=@tjikos/whatsover
bash packages/whatsover/scripts/install.sh
whatsover serve
```

The installer adds `~/.local/bin/whatsover`, linked to this checkout and its Bun
executable. Keep the checkout and installed dependencies available. It does not
start a service or change desktop configuration. Without installing, substitute
`bun run whatsover` for `whatsover` in the commands below.

In another terminal:

```sh
whatsover login
whatsover status
```

Scan the QR using WhatsApp on your phone → Settings → Linked devices → Link a
device. Login displays the QR on stderr and returns connection status as JSON on
stdout. Subsequent service starts reuse the persisted session. Ctrl+C disconnects
the service without unlinking the device.

## Send from scripts

```sh
whatsover send --to '+85255550123' --text 'Hello' --id 'example-job-001'
printf '%s' 'Message from a script' | whatsover send \
    --to '+85255550123' --stdin --id 'example-job-002'
whatsover result example-job-001
whatsover groups
whatsover members '120363000000000000@g.us'
whatsover send --to '120363000000000000@g.us' --text 'Hello group' --id 'example-group-001'
```

These are example destinations; replace them deliberately. `groups` lists actual
group names and JIDs for your linked account. Phone numbers must be international,
without spaces; exact individual, group and LID JIDs are also accepted. Contact
names are not resolved automatically. This first version sends text only.

`members GROUP_ID` reads the membership and available contact names without
sending messages. The result distinguishes communities and announcement groups.
It includes first/last name, country, phone number, original display name, source
labels, and WhatsApp ID. Names are a mechanically split display name and **must
be reviewed**; WhatsApp does not supply structured legal first/last names.
Country is the phone-number region, not residence or nationality. Hidden phone
numbers and unavailable names remain null. Contact names from account sync are
cached privately alongside the session; the API exports only the selected group.
Redirect membership exports only to private locations outside the repository.

Membership rows expose `saved_contact_name` and `profile_name` separately. Profile
names (shown with `~` in WhatsApp) are useful matching evidence even when no saved
contact exists. Whatsover accepts profile-name-only sync snapshots and live
contact updates; it does not import chat-message history. WhatsApp may not resend
an old profile-name snapshot to an already linked device, so this does not promise
an immediate complete directory. Screenshot observations can supply missing names
when matched to the exact exported phone number; preserve their source and avoid
automatically treating ambiguous initials as a confirmed identity.

To explicitly save an individual contact, pass a guarded JSON update on stdin:

```sh
printf '%s' '{"to":"+85255550123","firstName":"Synthetic","lastName":"Example","expectedName":"Synthetic"}' \
    | whatsover contact --stdin
```

This changes WhatsApp contact state and requests saving to the primary phone's
address book. The phone controls that final synchronization. A changed current
name causes a conflict; the operation does not send a chat message. Descriptions
and unknown-surname placeholders are rejected. WhatsApp's contact action exposes
first/full name, not a separate last-name field; the export retains both parts.
The TypeScript equivalent is `client.saveContact({ to, firstName, lastName, expectedName })`.

All command results are JSON on stdout. Errors go to stderr and exit nonzero.
Use `--stdin` to keep message text out of shell history and process arguments.

Every send requires a stable `--id` chosen by the calling script. Repeating the
same ID and content returns the stored outcome without another send. Reusing an ID
for different content fails. Choose a new ID only when you intend a new message.

Results have `id`, `to`, `messageId`, `state`, and `createdAt` (Unix milliseconds):

| State | Meaning |
| --- | --- |
| `submitted` | Baileys completed its send operation. This is **not** a recipient delivery/read receipt or a separately verified server acknowledgement. |
| `pending` | A send intent was saved and the operation is still in progress. |
| `uncertain` | The process or transport failed after recording the send intent. The message may already have been sent. Inspect the chat before deciding what to do. |

Network timeouts are also ambiguous: query `result` using the same ID. A missing
result can be retried with that same ID. Neither the service nor the CLI retries
uncertain messages under a new ID. This is duplicate suppression, not a guarantee
of exactly-once delivery. Reconnects use bounded exponential backoff; logged-out,
replaced, forbidden, and invalid sessions stop reconnecting.

## Library

Add `"@tjikos/whatsover": "workspace:*"` to a calling workspace's dependencies.

```ts
import { Whatsover } from '@tjikos/whatsover';
import { stateDirectory } from '@tjikos/whatsover/sqlite';
import { join } from 'node:path';

const whatsapp = new Whatsover({ socket: join(stateDirectory(), 'service.sock') });
await whatsapp.status();
await whatsapp.send({
    to: '+85255550123',
    text: 'Hello',
    id: 'my-job-2026-11-02',
});
```

For an HTTPS service, construct `Whatsover({ url, token })`. Remote plain HTTP,
redirects, URLs containing credentials, and ambiguous transport choices are
rejected. Unix socket transport requires Bun.

For an embedded client, use `WhatsAppSession` with a `Store` implementation:

```ts
import { WhatsAppSession } from '@tjikos/whatsover';
import { SQLiteStore } from '@tjikos/whatsover/sqlite';

const store = new SQLiteStore();
const session = new WhatsAppSession(store);
try {
    await session.start(); // Initiates connection; inspect status()/pairing().
    // Wait for status().state === 'connected' before session.sender.send(...).
} finally {
    await session.stop();
    await store.close();
}
```

The API client is the simpler choice when several scripts share one account.

## State and recovery

Default state is `$XDG_STATE_HOME/whatsover`, falling back to
`~/.local/state/whatsover`. `WHATSOVER_STATE_DIR` overrides it for both CLI and
service. `serve --state-dir PATH` overrides the service only; clients can select
its socket with `--socket PATH/service.sock`.

Credentials, Signal keys, outbound message bodies, and the send ledger stay in
this private directory. State under a Git checkout is rejected, including through
symlinks. Directories are mode 0700 and databases/socket 0600. The database is not
encrypted at rest; protect it as account credentials. No incoming chat history is
archived. Outgoing bodies are retained to support WhatsApp retry requests; this
version does not automatically expire the ledger or message bodies.

SQLite persists credential/key updates as they happen, using FULL synchronous
writes. A separate SQLite exclusive lock prevents two service processes from
using one state directory; the OS releases it on process death. Interrupted sends
become uncertain at restart. Back up the state with the service stopped, and never
run the same session database concurrently on two hosts.

If WhatsApp unlinks the account, `status` reports `logged_out`. Stop the service,
move the entire state directory to a private backup outside the repo, start with
a fresh directory, and run `login` again. A fresh directory has a fresh send
ledger: callers must not replay old jobs into it automatically. No destructive
reset or unlink command is provided.

## Service and scheduled windows

For a foreground nine-hour window:

```sh
whatsover serve --duration 32400
```

`--duration` is seconds and disconnects gracefully at the end. The service can
also be started by cron/systemd at each desired window. An optional user unit is
in [examples/whatsover.service](examples/whatsover.service); install it only if
you want a continuously running service:

```sh
install -Dm644 packages/whatsover/examples/whatsover.service \
    ~/.config/systemd/user/whatsover.service
systemctl --user daemon-reload
systemctl --user enable --now whatsover.service
```

The unit uses the installed `whatsover` wrapper. For scheduled operation, use a
timer and `--duration` instead of enabling the continuous unit.

## HTTP and containers

HTTP is opt-in and requires `WHATSOVER_TOKEN` of at least 32 characters. Set the
token through private environment configuration or a secret manager. Do not put
it into this repository or a command-line argument.

```sh
# WHATSOVER_TOKEN is already set in the private environment.
whatsover serve --host 127.0.0.1 --port 8789
WHATSOVER_URL=http://127.0.0.1:8789 whatsover status
```

All routes require `Authorization: Bearer <token>` over HTTP:

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/v1/status` | Connection state and linked identity |
| GET | `/v1/qr` | Current sensitive pairing QR string; never log/cache this |
| GET | `/v1/groups` | Group names and IDs |
| GET | `/v1/groups/:id/members` | Membership and available contact information |
| POST | `/v1/contacts` | Explicit guarded contact save |
| POST | `/v1/messages` | Send `{ "to": "…", "text": "…", "id": "…" }` |
| GET | `/v1/messages/:id` | Inspect the send ledger without exposing message text |

The service rejects browser Origin headers and sets `Cache-Control: no-store`.
Use HTTPS at the external boundary. For a conventional container, run Bun with
this package and its dependencies, bind to `0.0.0.0`, and mount a persistent
private state directory. The service must have outbound WhatsApp connectivity.

### Cloudflare boundary

This package does **not** provision or deploy Cloudflare resources. The included
SQLite store requires a durable local filesystem; putting it on Cloudflare
Containers' ephemeral disk would lose the session.

The exported asynchronous `Store` interface is the integration point for a
Durable Object adapter. That adapter and the Worker/container deployment are not
implemented here. It must persist authentication writes before resolving,
atomically claim request IDs, recover interrupted sends, and enforce a single
container owner per account. Durable Object SQLite is not a filesystem volume
that can be mounted into the container. Do not substitute shutdown-only snapshots
or periodic uploads for durable credential updates.

## Development

```sh
bun run check --filter=@tjikos/whatsover
bun run build --filter=@tjikos/whatsover
bun run --cwd packages/whatsover test
```

Tests use synthetic fixtures and temporary directories outside the repository.
They exercise auth persistence, crash locks, uncertain sends, reconnect behavior,
HTTP authorization, and CLI/library calls over real local sockets. They never
connect to WhatsApp. Live QR pairing and real delivery require separate testing
with a chosen account and destination.

Baileys is an unofficial client, pinned in the workspace lockfile. WhatsApp
protocol changes may require an update, and use of unofficial clients carries
account-restriction risk.
