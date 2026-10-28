> Historical engine/design notes moved from dotfiles. Use [mailtjik](../README.md) for the current `read` and `archive` interfaces. Engine modules now live in `backend/`.

# Annual email archives: operations

The `email-archives` command manages immutable account/year collections in
`type-drive:private/archives/mail/structured/`. Original imports remain intact.
The implementation is in `scripts/email_archive_annual.py`,
`scripts/email_archives.py`, `scripts/email_archive_live.py`, and
`scripts/email_archive_tui/`. The previous account-wide exporter is held for
the ongoing migration and is not the annual export entry point.

## Install

Run `bash scripts/install-email-archives.sh` from this repository. It builds
the pinned Bubble Tea/Lip Gloss tree selector and installs only this command
and its Python modules. Python uses the standard library; runtime dependencies
are age, zstd, rclone, and 7z for legacy 7z sources. Go is needed to build the
selector. This does not run a broad Chezmoi apply or change backup timers.

Keep private job directories, OAuth clients/tokens, provider configuration,
plans, indexes, manifests, restored mail and credentials outside Git. Jobs set
umask 0077; use private directories for restored output too. The archive uses
the recipient/identity from `~/.config/document-archive-policy.json` and Drive
configuration at `~/.config/private-storage/drive.conf`.

## Export a completed historical import

```sh
email-archives --workdir PRIVATE_ANNUAL_JOB start \
  --checkpoint PRIVATE_IMPORT/index.complete.json \
  --config PRIVATE_IMPORT/config.json
email-archives --workdir PRIVATE_ANNUAL_JOB watch
```

`--wait-checkpoint` makes the worker wait for up to 24 hours for the completed
index marker. It never opens an index still being written. The worker pins
its Python code, holds an exclusive job lock and survives terminal closure.
Ctrl-C in `watch` only detaches. `status` reads one progress snapshot.
`export` runs the same engine in the foreground. Resume with the same command
and job directory; a different input must use a new job directory.

Stage progression: waiting for index → scanning raw sources → local part
validation → upload/readback → reader views and attachments → partition
indexes → catalogue publication → complete. `status.json` contains stage,
processed occurrences, elapsed time and the current part size/count. It does
not claim an ETA without a reliable denominator. Raw source occurrences can
exceed the unique-message count.

The raw scan uses bounded writers and temporary encrypted ciphertext. Reader
views are generated one account/year at a time from verified raw parts to
limit open compression processes. Ciphertext staging is checked for available
space. A source checkpoint is committed only after all its raw writers seal.
Interrupted parts remain unpublished; committed objects are reused. No source
archive is modified, moved or removed.

Every object is decrypted locally and its content hashes and locators checked.
Raw EML is reparsed; reader MBOX records are reparsed and counted. Upload uses
`rclone copyto --immutable` directly, followed by a streamed remote ciphertext
SHA-256 comparison. Only verified objects enter the ledger. The encrypted
catalogue is published last. `published.json` is the local completion receipt;
a systemd exit or an empty FUSE queue is not a completion receipt.

## Find and read mail

```sh
email-archives --workdir PRIVATE_CACHE catalogue
email-archives --workdir PRIVATE_CACHE catalogue --object CATALOGUE_OBJECT
email-archives --workdir PRIVATE_CACHE list
email-archives --workdir PRIVATE_CACHE select --output PRIVATE_RESTORE_PLAN.json
email-archives --workdir PRIVATE_CACHE --selection-plan PRIVATE_RESTORE_PLAN.json \
  search 'invoice'
email-archives --workdir PRIVATE_CACHE --year 2024 --account ADDRESS \
  fetch RAW_SHA256 --output RESTORED.eml
email-archives --workdir PRIVATE_CACHE --year 2024 --account ADDRESS \
  attachment ATTACHMENT_SHA256 --output RESTORED_ATTACHMENT
email-archives --workdir PRIVATE_CACHE --selection-plan PRIVATE_RESTORE_PLAN.json \
  restore --output PRIVATE_RESTORE_DIRECTORY
```

Global options precede the operation. Repeat `--year` and `--account` as needed.
The tree supports arrows, Space, search with `/`, mixed parent selection,
select-all/clear, and a save preview. Output plans must be new files.

Search downloads only the selected encrypted SQLite indexes. Fetch downloads
the exact encrypted raw part, decrypts the entire part to authenticate it,
locates the requested member and compares its raw SHA-256. An age/zstd stream
cannot be remotely sought by its plaintext offset. Parts target 128 MiB of
uncompressed content; a single larger message or attachment gets its own part.

Open restored EML in Thunderbird or another standard mail client. MBOX reader
parts can be imported into Thunderbird; they preserve text/HTML and inline
images, with non-inline attachments represented by filenames and SHA-256
references. The full raw EML always contains the original attachments. No
custom reader is required. The private `folders.json` maps opaque restored
directories to original folder names. Existing output files are never replaced.

Reader restoration follows this catalogue's record locators. An older immutable
part can contain a record superseded by later folder information; those obsolete
records are excluded by the current index. Old encrypted generations remain
available for historical restoration.

## Add another year or source

Create a new completed input checkpoint with the unchanged full canonical
address-priority list. Start a new job with the published previous ledger:

```sh
email-archives --workdir PRIVATE_NEW_JOB start \
  --checkpoint PRIVATE_NEW_IMPORT/index.complete.json \
  --config PRIVATE_NEW_IMPORT/config.json \
  --base-ledger PRIVATE_PREVIOUS_JOB/ledger.sqlite
```

The base must be a finished, published generation. Its raw SHA-256 registry,
objects and memberships are copied into the new private ledger. Existing raw
messages retain account/year assignments, exact duplicates add provenance,
and differing bytes sharing a Message-ID remain separate. Late arrivals add
objects to their original years. Attachment deduplication is per account/year.
Changed membership evidence updates reader selections without deleting old raw
objects. The new complete catalogue references both old and new objects.

The encrypted catalogue contains a recoverable full ledger as well as small
per-partition indexes. The full ledger currently serves as the cumulative hash
registry; it is larger than a dedicated hash-only registry. New generations
must use the same destination and recipient. Backup snapshot retention is
separate from retaining historical mail years.

## Connect live mail

Logical archive addresses and authenticated mailbox logins are separate.
Choose the real active provider/mailbox locally. No live account is considered
connected until its login and metadata enumeration succeed.

For Gmail/Workspace, create a Desktop OAuth client with Gmail API enabled and
save its client JSON privately. Authenticate locally:

```sh
email-archives live --workdir PRIVATE_LIVE_JOB --connection PRIVATE_CONNECTION.json \
  auth-gmail --client PRIVATE_GOOGLE_DESKTOP_CLIENT.json
email-archives live --workdir PRIVATE_LIVE_JOB --connection PRIVATE_CONNECTION.json \
  inventory --config PRIVATE_PRIORITY_CONFIG.json
email-archives select --inventory PRIVATE_LIVE_JOB/inventory.json \
  --output PRIVATE_CAPTURE_PLAN.json
email-archives live --workdir PRIVATE_LIVE_JOB --connection PRIVATE_CONNECTION.json \
  preview --plan PRIVATE_CAPTURE_PLAN.json
email-archives live --workdir PRIVATE_LIVE_JOB --connection PRIVATE_CONNECTION.json \
  start-capture --plan PRIVATE_CAPTURE_PLAN.json
email-archives --workdir PRIVATE_LIVE_JOB watch
```

Gmail requests `gmail.readonly` using a browser/loopback OAuth flow with PKCE.
Refresh tokens are stored only in the private connection file. Reuse the same
connection and live job directory for subsequent inventories; the job is bound
to that connection to prevent cross-mailbox checkpoint reuse.

Inventory enumerates metadata for all provider IDs, then replays Gmail history
changes. Annual inventories deliberately re-enumerate metadata; this catches
late historical arrivals and changed label policies even when old history has
expired. HTTP 404 during enumeration/history triggers one full reconciliation
retry. Persisted body captures survive retries; metadata enumeration can repeat
after an interruption. Missing or inaccessible messages stop the job without
claiming full coverage.

Year means original Date converted to UTC; missing/naive/invalid Date falls
back to Gmail internalDate or IMAP INTERNALDATE, then `unknown-date`. The
partition index records the basis and provider evidence. Filtering always uses
the full private canonical address-priority list before selecting addresses.
Unresolved messages have their own selectable collection.

Checked labels under **Exclude labels** omit messages carrying any selected
label. This union is applied before body downloads, including All Mail. Gmail
Spam/Trash are excluded by default; `inventory --include-spam-trash` makes
them selectable. Labels are rechecked before each raw download and after
capture. Preview counts distinct provider IDs and estimated bytes; exact raw
deduplication occurs after body download. Label counts are never added as if
they were disjoint. Gmail Archive is the residual folder after other retained
folder memberships; starred/unread/important flags alone do not suppress it.

Capture produces a private immutable `capture.tar`, completed input SQLite,
selection policy and `index.complete.json`. `capture.complete.json` records
their directory. Start the annual exporter with that checkpoint/config and
the preceding published `--base-ledger`. Capture is an observed time interval,
not an atomic server snapshot: a later inventory catches arrivals after the
inventory cut-off. Exclusion changes do not erase already archived history.

For another provider, configure IMAP explicitly:

```sh
email-archives live --workdir PRIVATE_LIVE_JOB --connection PRIVATE_CONNECTION.json \
  connect-imap --host IMAP_HOST --username MAILBOX_LOGIN \
  --secret-file PRIVATE_APP_PASSWORD_FILE --mailbox INBOX --mailbox Sent
```

TLS is mandatory; mailbox selection is read-only and downloads use
`BODY.PEEK[]`. Checkpoints include mailbox, UIDVALIDITY and UID. For an existing
provider OAuth access-token file use `--auth oauth2`; generic IMAP does not
implement the provider's token refresh flow. Mailbox names are explicit server
names (including modified UTF-7 where required). Generic IMAP exclusions are
mailbox exclusions: it lacks Gmail's cross-folder label/message identity.
Use the Gmail API for Gmail label-union exclusion semantics.

## Current validation boundary

Python syntax and the Go build can be checked independently of live accounts.
The export worker performs actual message/attachment hash checks, MBOX
reparsing, encryption/decryption and remote ciphertext readback as it runs.
CLI restoration uses the same authenticated parts and exact locators. Live
provider behavior requires real login and a completed inventory/capture.
No annual timer is installed until active mailbox/provider mappings and
authentication are available. No original cleanup is part of this rollout.

### Reader startup progress

On first search, Mailtjik downloads the selected search indexes, decrypts and verifies them, then searches them. The reader displays verified indexes ready, downloaded bytes, elapsed time, and a download ETA after enough transfer data is available. The ETA covers downloading only; verification and searching follow. Later searches check the cached indexes and show search progress without a download ETA.
