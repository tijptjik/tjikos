> Historical engine/design notes moved from dotfiles. Use [mailtjik](../README.md) for the current `read` and `archive` interfaces. Engine modules now live in `backend/`.

# Annual mail archive and selective import design

Status: implementation added on 2026-10-25; see the
[operations guide](email-archives-operations.md) for commands and validation
boundaries. The existing indexer can finish unchanged. Its previous export
layout is held using a private `export.hold` file in the migration working
directory. Publication and live authentication must be reported from actual
completion receipts, independently of code availability.

## Partitioning and retrieval

Use one logical collection per canonical address and year, with independently
compressed and age-encrypted parts. Keep the existing address-priority rule
and an explicit unresolved collection. Selecting fewer accounts must not
change attribution priority.

Target 128 MiB of uncompressed content per part, closing parts only between
messages or attachments. A single object larger than the target gets its own
part. The target is not a hard limit on encrypted file size. This is smaller
than the current exporter's 512 MiB target. Yearly grouping alone would not
make a very large single compressed MBOX efficient to retrieve.

Illustrative physical layout under `private/archives/mail/structured/`:

```text
catalogues/<generation>.json.zst.age
2024/<account-id>/<generation>/
  index.sqlite.zst.age
  manifest.jsonl.zst.age
  raw/part-0001.tar.zst.age
  attachments/part-0001.tar.zst.age
  mailboxes/<folder-id>/part-0001.mbox.zst.age
unknown-date/<account-id>/<generation>/...
```

The UI displays canonical addresses and folder names from encrypted metadata;
opaque IDs in object paths remain stable across generations. Raw EML messages
preserve source bytes and MIME attachments. Standard MBOX reader views retain
text/HTML and inline images, with separately stored ordinary attachments.

Use a small collection catalogue for partition discovery and counts, plus a
searchable SQLite index for each account/year. Cache those indexes privately;
searching across years may fetch additional indexes, but does not fetch mail
bodies. A compact encrypted raw-hash registry supports deduplication across
imports. Keep full provenance and differing messages sharing a Message-ID.

Add explicit output-object locators to the index: raw SHA-256, object ID/path,
tar member name, uncompressed offset and length, ciphertext hash and size,
generation, account, year, and object kind. Record attachment and reader-view
locators too. The current source locators and mailbox paths alone are not
sufficient to locate a raw message in output parts.

Retrieval becomes: select/search account/year index, download the identified
encrypted part, authenticate/decrypt/decompress it, verify the message hash,
and open an EML or restore a standard MBOX. Compression/encryption means a
plaintext offset does not provide random access to the remote ciphertext;
the part bounds the work. A raw EML already contains its original attachments.

Deduplicate extracted attachments within each account/year for independent
year restoration. The same attachment can occur in different years; accepting
that duplication avoids a year depending on an unrelated year's object pack.
Exact raw message duplicates remain globally registered and retain all source
and folder memberships.

## Which year

Default archive year: the original message Date converted to UTC when it has
a valid timezone. If unavailable, use a recorded provider timestamp such as
Gmail internalDate or IMAP INTERNALDATE. Otherwise use `unknown-date`, rather
than extraction time, archive-file mtime, or an assumed timezone. Store both
timestamps and `year_basis`. An already archived exact raw hash keeps its
existing assignment; conflicting later evidence is recorded for review.

The selector must label this as **message year**. Gmail date queries use
provider search dates and calendar strings are interpreted in PST. They are
not proof of coverage by original header Date. For exact message-year selection,
build/reuse a metadata inventory across eligible messages before downloading
selected bodies. An optional **received year** mode can use provider time and
epoch-second query bounds, with its different semantics recorded in the plan.
Boundary padding alone cannot handle arbitrarily delayed or migrated mail.

## Adding annual archives

Seal immutable parts and publish a new catalogue generation only after uploads
and verification succeed. Future runs add new parts and memberships. An exact
raw message already registered does not need another stored raw copy. A late
arrival dated in an older year adds a delta generation to that year. Updating
labels updates provenance/views without changing preserved message bytes.

Record import policy, source identity, timestamps, source hashes or provider
IDs, exclusions, parser version, and completion evidence. Keep incomplete runs
distinct from published generations. Existing historical sources are imported
without applying newly chosen live-mail exclusions retroactively.

The 1 daily / 1 weekly / 1 monthly backup preference applies to recovery
snapshots, not deletion of historical mail years.

## Selector and terminal UI

Proposed entry point: `email-archives`. Separate operations:

- **Import live mail**: choose years, canonical addresses, source connections,
  and labels to exclude, then inspect the resulting plan.
- **Import existing backup**: inspect source and date coverage before adding
  messages to the same account/year collections.
- **Browse / restore**: select archive years/accounts, search their indexes,
  and fetch messages, attachments, or complete MBOX views.
- **Status / resume**: reconnect to a durable running or interrupted job.

The selection tree expands Year → Address → Excluded labels. Checkbox meaning
is explicit: year/address checkboxes include data; checked labels under the
"Exclude labels" heading exclude matching messages. Parent nodes support
mixed selection, and a parent action applies to its descendants. Provide
arrow-key navigation, Space to toggle, search, select-all/clear, and a plan
preview showing distinct message counts and estimated bytes. Label counts
overlap and must not be summed as independent messages. Sanitize provider
labels and other untrusted text before rendering terminal escape sequences.

Gum v2.0.2 is already installed locally. Gum `choose --no-limit` and `filter`
offer flat multiselect; `file` selects filesystem entries. No generic
hierarchical checkbox selector was found in its documented commands.
Huh supplies forms and flat MultiSelect fields. Lip Gloss has a tree renderer,
but rendering a tree does not implement selection state or keyboard behavior.

Use a small Go front end built with Bubble Tea for the interactive tree,
Lip Gloss for rendering, and Bubbles progress/spinner components. Keep the
existing Python ingestion/export logic behind a versioned JSON plan/events
interface. The UI need not become a mail reader: EML/MBOX remain readable in
Thunderbird and other standard clients.

## Exclusions and Gmail Archive

Discover label IDs and display names from each connected mailbox. Gmail labels
are overlapping memberships, not disjoint directories. Store stable provider
label IDs, retain display-name history, and distinguish Gmail system labels
and categories from user labels. Nested names can be displayed as a tree;
the plan records exactly which label IDs a parent selection expands to.

Compute selected message IDs minus the union of messages carrying excluded
labels **before downloading bodies**. Exclusion wins even when a message also
appears in All Mail or an included label. Read current label metadata again
before capture and record changes observed during the run. Metadata enumeration
is not an atomic provider snapshot; replay changes before finalizing coverage.

All Mail is the candidate universe, not a second physical copy. Gmail API
enumeration does not require an invented `ALL_MAIL` label ID. Make Spam/Trash
inclusion explicit. Newsletter labels are selected by the user, not inferred
from names or sender guesses. Unchecking a label on a later run can import
previously excluded mail; it must trigger appropriate metadata reconciliation.

After exact deduplication, reader views show selected folder/label memberships.
`Gmail Archive` contains retained messages not represented in another selected
folder view. Unread, starred, and important flags alone do not remove a message
from Gmail Archive. Excluded messages must not reappear through this residual
view. Historical originals and already preserved raw messages are not deleted
by a later import policy change. Missing historical label data is shown as
unknown; absence of an exported label is not evidence the message lacked it.

## Live sources and authentication

For Gmail/Workspace, prefer the Gmail API using a desktop OAuth flow and
`gmail.readonly`. Enumerate messages once, obtain per-message labels/metadata,
and fetch selected messages using `format=raw`. Base64url decoding gives the
provider's raw message bytes; hash and preserve them without reserialization.
Track provider account ID, message/thread IDs, history ID, and source dates
separately from canonical archive address. A mailbox login and an address
identity are different concepts. Preserve the user's attribution priority and
keep messages whose headers cannot establish a priority address unresolved.

Use the browser and loopback OAuth callback with PKCE where supported. Keep
OAuth clients/tokens in the private credential store; recover credentials via
Bitwarden, not Git, manifests, command arguments, or chat. Drive authorization
does not imply Gmail authorization. Live connection cannot be claimed until
the user has authenticated the selected mailbox and discovery has succeeded.

Use Gmail history for recent incremental runs, but an annual job cannot assume
a year-old history cursor remains valid. On stale-cursor HTTP 404, re-enumerate
metadata and reconcile provider IDs; download raw bytes only for selected IDs
that are new or require verification. Periodically reconcile labels, changed
exclusion policies, missing IDs, and late historical arrivals.

For other providers, use TLS IMAP with OAuth where available, or a recoverable
provider-approved app password. Open mailboxes read-only and fetch full
`BODY.PEEK[]`. Durable identity includes mailbox + UIDVALIDITY + UID; sequence
numbers are not checkpoints. UIDVALIDITY changes require reconciliation.
Gmail IMAP offers X-GM-MSGID/X-GM-LABELS when its API cannot be used. Generic
IMAP providers may not expose Gmail-equivalent labels; explain that limit.

## Durable progress and validation

Use a persistent private job database and per-run lock. A user-systemd worker
owns the job; the TUI observes JSON events and can detach/reconnect. Explicit
pause/cancel finishes or records an interrupted part and checkpoints work.
Never equate a missing service or a drained upload queue with a verified
complete archive.

Track discovery, metadata inventory, excluded/selected IDs, raw download,
hashing/deduplication, attachment extraction, encryption, upload, and restore
verification separately. Display counts, bytes, transfer rate, retries, errors,
and an ETA only when a useful denominator exists. Provider size estimates are
labelled as estimates. Retry transient failures with bounded backoff and record
per-message errors; a missing message or failed shard prevents a complete
coverage claim. Commit completion only after the corresponding durable data.

Before publishing, verify source raw hashes, output message/attachment hashes,
index references, distinct counts, and policy decisions. Re-read uploaded
ciphertext, compare checksums, decrypt and reparse parts, and restore/open an
EML plus a standard MBOX. Test interruption/restart, overlapping labels,
cross-account priority, missing/ambiguous dates, differing bytes with the same
Message-ID, annual reruns, late arrivals, and stale provider cursors.

## Implementation sequence

1. Finish the active persistent index; retain its checkpoint and existing
   source archives. Keep the previous export stage held.
2. Build account/year planning, bounded parts, exact object locators, and the
   encrypted collection catalogue. Reuse the completed source index.
3. Implement selective archive retrieval and verify a year/account restore.
4. Add the Bubble Tea selector and durable JSON job interface.
5. Connect user-confirmed active mailboxes, discover real labels, preview the
   selection policy, and run the first selective live import.
6. Verify idempotent reruns and schedule annual captures for active accounts.

The missing user input for live sources is the active address/provider mapping;
authentication must take place through the actual account login flow.

## Primary references

- [Gum commands](https://github.com/charmbracelet/gum)
- [Huh fields](https://github.com/charmbracelet/huh)
- [Bubble Tea](https://github.com/charmbracelet/bubbletea)
- [Lip Gloss tree renderer](https://pkg.go.dev/github.com/charmbracelet/lipgloss/tree)
- [Bubbles progress components](https://github.com/charmbracelet/bubbles)
- [Gmail message filtering and date semantics](https://developers.google.com/workspace/gmail/api/guides/filtering)
- [Gmail message resource and raw format](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages)
- [Gmail synchronization and stale history](https://developers.google.com/workspace/gmail/api/guides/sync)
- [Gmail scopes](https://developers.google.com/workspace/gmail/api/auth/scopes)
- [Desktop OAuth](https://developers.google.com/identity/protocols/oauth2/native-app)
- [Gmail IMAP extensions](https://developers.google.com/workspace/gmail/imap/imap-extensions)
