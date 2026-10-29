> Historical engine/design notes moved from dotfiles. Use [mailtjik](../README.md) for the current `read` and `archive` interfaces. Engine modules now live in `backend/`.

# Email archive format

The [annual archive implementation and commands](email-archives-operations.md)
add account/year collections, smaller retrievable parts, a Charm tree selector,
read-only live capture, and immutable delta generations. See the
[design](email-archive-annual-design.md) for the rationale. The rest of this
page documents the earlier account-wide format and runner; use the annual
operations guide for new exports. The old migration export remains held.

The structured archive is stored under Google Drive `private/archives/mail/structured/`.
Every data object is compressed with zstd and encrypted with age before upload.
Drive filenames use opaque account and mailbox IDs; the encrypted SQLite index
and JSONL manifest map those IDs to addresses and original source paths.

## Running a migration

Keep the private configuration, working SQLite database, manifests, logs, and
completion markers in persistent storage such as
`~/.local/state/email-archive/`. Use directory permissions 0700 and file
permissions 0600. A reboot clears `/tmp`; it must not hold the only index.

Run `scripts/email_archive_run.py --workdir PRIVATE_DIRECTORY --config PRIVATE_CONFIG`
from the repository root in the host user session. The runner waits directly
for the index process to exit successfully, checks SQLite integrity and any
configured baseline counts, and only then starts export. It does not infer
completion from a transient systemd unit disappearing. A private lock prevents
two runner instances from using the same working directory.

The `index.complete.json` checkpoint permits reuse of a completed index when
source sizes and modification times still match. Incomplete attempts are kept
in separate directories. `upload.complete.json` means export finished and the
mount upload queue drained; it does not mean restoration validation passed.
The runner requires the writable `gi-private` FUSE mount at
`~/.mnt/google/private` before export. Launch it with a user systemd service to
survive terminal closure, and retain its logs on persistent storage.

Each canonical account has three encrypted data collections:

- `mailboxes/`: standard MBOX files, one per original mailbox source. Reader
  views retain text and inline images, and replace non-inline MIME attachments
  with a filename, size, and SHA-256 reference.
- `raw-messages.tar.zst.age`: one exact preserved message payload per unique
  raw-message SHA-256, including its original MIME attachments.
- `attachments.tar.zst.age`: non-inline attachment files, deduplicated by
  content SHA-256. Inline images remain in the MBOX view and raw message.

Large MBOX views are split into numbered parts. Each part is a valid MBOX file;
import the parts in order or concatenate the decrypted/decompressed parts when
a single file is preferred. The manifest records the source mailbox and the
part that contains each reader-view message.

The index collection contains the sensitive `manifest.jsonl` and
`email-index.sqlite`. SQLite FTS5 indexes subjects and header addresses, not
bodies. Message-ID is searchable but is not unique. Exact raw-message hashes
drive deduplication; all source and folder memberships remain in the index.
The Gmail All Mail view is named `Gmail Archive` and excludes messages that
also belong to another folder for the same account. Those memberships remain
available in the manifest and index.

The age identity is the managed `~/.keys/age.txt` key, recoverable from
Bitwarden. The identity was checked against the configured archive recipient.

## Restore and read

Decrypt an MBOX reader view and pipe it to a standard MBOX file:

```sh
age -d -i ~/.keys/age.txt account-mailbox.part-0001.mbox.zst.age \
  | zstd -d > restored.mbox
```

Import `restored.mbox` into Thunderbird using an MBOX importer, or open it with
another MBOX-compatible mail client. No custom mail interface is required.

Restore the raw messages or attachment collection with:

```sh
age -d -i ~/.keys/age.txt raw-messages.tar.zst.age \
  | zstd -d | tar -xf -
```

The SQLite index can be opened with `sqlite3` or a graphical SQLite browser.
For example, search the FTS5 subject and header-address index with:

```sql
SELECT m.subject, m.normalized_utc, m.from_header, m.to_header, m.sha256
FROM message_search s
JOIN messages m ON m.rowid = s.rowid
WHERE message_search MATCH 'invoice';
```

Keep the original source archives until the encrypted exports have been
reparsed, their message and attachment hashes have been compared with the
index, and a restore has been tested.
