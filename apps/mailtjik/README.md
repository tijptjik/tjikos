# mailtjik

![Turborepo](https://img.shields.io/badge/Turborepo-EF4444?style=flat-square&logo=turborepo&logoColor=white)
![Bun](https://img.shields.io/badge/Bun-14151A?style=flat-square&logo=bun&logoColor=white)
![Go](https://img.shields.io/badge/Go-00ADD8?style=flat-square&logo=go&logoColor=white)
![Python](https://img.shields.io/badge/Python-3776AB?style=flat-square&logo=python&logoColor=white)
![Charm](https://img.shields.io/badge/Charm-Bubble_Tea_%C2%B7_Lip_Gloss_%C2%B7_Glamour-FF75B5?style=flat-square)
![SQLite](https://img.shields.io/badge/SQLite-003B57?style=flat-square&logo=sqlite&logoColor=white)
![age](https://img.shields.io/badge/age-encrypted-89DCEB?style=flat-square)
![Kitty](https://img.shields.io/badge/Kitty-inline_graphics-CBA6F7?style=flat-square)

**mailtjik preserves your email in encrypted annual archives and makes it easy to find and read again in the terminal.**

![mailtjik: search, read and select annual archives](docs/demo.gif)

*Recorded with [Charm VHS](https://github.com/charmbracelet/vhs), using 96 synthetic messages. The recording uses ANSI image thumbnails; Kitty displays higher-resolution images through its native graphics protocol.*

`tooltjik` is a Bun/Turborepo monorepo for personal CLI applications. Its first application is `apps/mailtjik`: a Go/Charm interface backed by the existing Python mail preservation engine.

## Features

- **Two modes:** `mailtjik read` and `mailtjik archive`. Running `mailtjik` opens a mode chooser.
- **Find conversations:** search a participant's name or address; restrict matches to **From, To, Cc or Bcc**; combine with subject, account and inclusive start/end years.
- **Counts everywhere:** catalogue totals, account/year coverage, exact search counts, paginated results and capture previews. Pending publication/search is explicitly labelled rather than shown as a completed count.
- **Read comfortably:** Glamour renders text and HTML converted to Markdown. Original headers, attachment counts and image counts stay visible.
- **Embedded images in Kitty:** PNG, JPEG, GIF (first frame) and WebP appear in an inline pane below the message. Images fit both the available terminal width and height, resize with the terminal, and can be cycled or hidden. No remote images or tracking pixels are fetched.
- **Export originals:** save a verified raw EML or its ordinary attachments for use in another mail client.
- **Archive by address and year:** independently compressed, age-encrypted parts targeting 16 MiB of uncompressed content; one oversized message or attachment may exceed that target. Existing 128 MiB parts remain readable.
- **Preserve evidence:** exact raw SHA-256 deduplication, all source/folder memberships, attachments, aliases and attribution evidence. Different messages sharing a Message-ID stay separate.
- **Choose what to capture:** a year → address → excluded-label tree. Exclusion wins across all Gmail labels, including All Mail. `Gmail Archive` is the retained residual view.
- **Read-only live capture:** Gmail desktop OAuth, metadata inventory and resumable body downloads; TLS IMAP is available through private provider configuration.
- **Durable workers:** indexing, capture and encrypted export survive closing the TUI. New yearly generations reuse the existing raw registry.
- **Standard recovery formats:** EML, MBOX, SQLite and JSONL. Local decryption/content checks and remote ciphertext readback precede catalogue publication.

## Install

Requirements: Linux, **Bun 1.4+**, **Go 1.24+**, **Python 3.11+**, `age`, `zstd`, `rclone`, and a user systemd session for background workers. Legacy 7z imports also need `7z`. Kitty is needed for inline graphics; other colour terminals display an ANSI thumbnail.

From the `tooltjik` checkout:

```sh
bun install
bun run install:local
```

The installer builds through Turbo and installs:

```text
~/.local/bin/mailtjik
~/.local/lib/mailtjik/*.py
```

Ensure `~/.local/bin` is in `PATH`. To build without installing, run `bun run build`, then `apps/mailtjik/dist/mailtjik`.

For a completely local demonstration, with no personal archive or credentials:

```sh
mailtjik read --demo
mailtjik archive --demo
```

To regenerate the GIF and screenshots, install `vhs`, `ffmpeg`, `ttyd`, and a Chromium-compatible browser, then run:

```sh
bun run demo
```

The tape is [`apps/mailtjik/docs/demo.tape`](docs/demo.tape). It always uses `--demo`; it never records personal mail. The requested `READMD.md` spelling is also provided as a link to this document.

## Setup

### 1. Choose private working locations

Run `mailtjik archive`, select **Setup**, and set the archive directory, prepared import directory, source configuration, encryption policy, remote, and any live connection paths. **Tab** moves between fields; **Ctrl-S** saves to `~/.config/mailtjik/config.json`. Use `--config /private/path/config.json` to select another configuration.

All mail, indexes, manifests, OAuth clients/tokens, credentials and restored files belong outside the repository. The tool uses private directory/file permissions. Working data must be on persistent storage with enough free space for the index and temporary ciphertext.

Example private application configuration:

```json
{
  "workdir": "/home/you/.local/state/mailtjik/archive",
  "import_workdir": "/home/you/.local/state/mailtjik/import",
  "source_config": "/home/you/.config/mailtjik/sources.json",
  "policy": "/home/you/.config/mailtjik/encryption.json",
  "remote": "type-drive:private/archives/mail/structured",
  "live_workdir": "/home/you/.local/state/mailtjik/live",
  "connection": "/home/you/.config/mailtjik/gmail.json",
  "client": "/home/you/.config/mailtjik/google-desktop-client.json",
  "base_ledger": ""
}
```

Configure the rclone remote in `~/.config/private-storage/drive.conf`. The destination is ordinary Drive storage: files are compressed/encrypted **before** upload. An rclone crypt mount is not required.

The encryption policy contains a public age recipient and a path to its private, recoverable identity:

```json
{
  "recipient": "YOUR_AGE_RECIPIENT",
  "identity": "~/.keys/mail-archive.txt"
}
```

### 2. Prepare existing archives

Create the private source configuration, keeping the **full, stable priority order** for logical addresses:

```json
{
  "priority_addresses": ["alex@example.org", "studio@example.org"],
  "source_paths": [
    "/private/retained-backups/thunderbird.zip",
    "/private/retained-backups/takeout.tgz"
  ]
}
```

Source formats include Thunderbird/Takeout ZIP, tar/tar.gz/tgz, 7z, MBOX and EML. The first matching canonical address in From/Sender/To/Cc/Bcc wins. Selecting fewer addresses later does not change that order. Messages without a match stay in an explicit unresolved collection.

In **Archive**:

1. Select **Prepare existing backups** to index retained sources and create a durable completion checkpoint.
2. Select **Publish annual archive** to start the exporter. It waits for the completed index, validates sources, preserves raw messages and builds reader views and partition indexes.
3. Watch the **Workers** panel. Publication is complete only when the worker writes `published.json`; an idle upload queue alone is insufficient.

Source files are never removed, relocated or overwritten. The reader does not open the index while preparation is still writing it. New source sets use new import/job directories.

Year means the original Date converted to UTC, with provider timestamp fallback and an explicit `unknown-date` collection. Date-less mail remains visible when neither year bound is set.

### 3. Add live mail

Set a private Google **Desktop OAuth client** JSON and a new connection file path in Setup, then choose **Connect a Gmail mailbox**. Complete the browser login granting `gmail.readonly`.

Next choose **Inventory live mail**, then **Select and capture**. The tree shows candidate counts; the final preview computes distinct selected provider IDs after excluded-label unions. Spam and Trash are omitted by the default inventory. Raw deduplication follows the body download.

Capture rechecks labels and records provider identities separately from logical archive addresses. Generic IMAP can use a private connection file with `provider`, `id`, `host`, `username`, `secret_file`, `auth` and explicit `mailboxes`; its exclusions apply to mailboxes, not Gmail's cross-folder label identity. Gmail's API is the preferred route for Gmail label exclusions.

After capture finishes, choose **Publish captured mail** to create a new annual generation based on the current published ledger. When it finishes, set the archive directory to the new generation shown by the worker, or fetch its catalogue into a separate reader cache. Late arrivals go into their original year. Previously preserved raw mail is retained when an exclusion policy changes.

## Use

```sh
mailtjik             # choose Read or Archive
mailtjik read        # search and read
mailtjik archive     # prepare, select, preserve and monitor
```

### Find an email

1. Enter a **participant** name or email address and optionally a **subject** fragment.
2. Use **Tab** to reach **Role**; **Space/Left/Right** cycles `any`, `from`, `to`, `cc`, `bcc`.
3. Set the inclusive **start year** and **end year**, and optionally choose one account.
4. Press **Enter** to search. The exact match count and selected-scope available count appear above results.
5. Use **↑/↓** and **Enter** to read a message. **n/p** pages through results; **c** opens all account/year counts, including counts from the last search.

Search matches case-insensitive substrings. Participant and subject filters are combined with AND. Search downloads the selected indexes; opening a message fetches only its bounded encrypted part, authenticates it and verifies the preserved raw hash.

Missing indexes are fetched with up to four concurrent downloads and cached privately.
Later searches reuse those indexes. Opening a message downloads only its containing
part; subsequent reads reuse the verified local EML. A cold read of an older
128 MiB part can take longer than a new 16 MiB part. Neither requires downloading
an entire annual archive. Network throughput still determines cold-read latency.

Reader MBOX parts pool messages within each account/year. Folder memberships are
index references to record offsets, so a message in many folders is stored once.
The MBOX itself contains pooled messages; original folder recovery uses the index
memberships. Export progress includes messages and raw objects processed during
derivation; completed partition checkpoints remain separate resume evidence.

Messages that cannot be serialized into a reader view are preserved exactly in
the private job's `review/malformed/<raw-sha256>.eml`, with a JSON provenance
record beside each file. An encrypted `quarantine` collection and the exported
`review_messages` index table preserve these for later inspection. Reader MBOX
views contain an explicit review placeholder in the original folders. Original
raw messages and attachments remain preserved. Storage and checksum failures
still stop publication; quarantining a malformed header does not hide them.

### Read and export

| Key | Action |
| --- | --- |
| ↑ / ↓, PgUp / PgDn | Scroll the rendered email |
| `[` / `]` | Previous / next embedded image |
| `i` | Show / hide the inline image pane |
| `e` | Export the original raw EML |
| `a` | Save ordinary attachments separately |
| Esc | Return to results |

Exports are placed in a new private directory below the configured archive's `restored/` folder. Originals are not rewritten. EML opens in Thunderbird and other standard mail clients; older MBOX recovery instructions and the format design are retained in [`apps/mailtjik/docs/`](docs/).

Kitty images are restricted to the reserved pane and resized again when the terminal changes. Images beyond the display decoding limit remain in the original EML. Other terminals, and tmux sessions without Kitty passthrough, show ANSI thumbnails instead (captions when `NO_COLOR` is set). Physical Kitty display acceptance remains separate from the VHS recording.

### Monorepo layout

```text
apps/mailtjik/
  cmd/mailtjik/       Go reader/archive interface and Kitty display
  internal/archiveui/ hierarchical selection model
  backend/           Python preservation engine and private JSON bridge
  docs/              VHS tape, demo and format documentation
  scripts/           build and static checks
scripts/install.sh   local installation
package.json         Bun workspaces + Turbo tasks
turbo.json           build/check/demo task graph
```

`bun run check` checks Python syntax; `bun run build` compiles the application. Existing engine tests were moved with the source and are not part of these commands. Running archive jobs retain pinned code snapshots and are not upgraded in place by installation.
