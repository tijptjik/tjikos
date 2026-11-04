# Gmailtjik

A Rust + GPUI desktop Gmail client, with TjikOS's rose-punk palette. This uses
GPUI Kit's native inputs and GPU renderer; it does not use Quickshell/Qt.

The first version includes a label sidebar, a virtualized inbox, a readable
message view, Gmail search with pagination, and a plain-text composer with To,
Cc, Bcc, reply threading, local draft recovery, Gmail draft saving, and sending.
Archive, move to Trash, star, and read/unread changes go through Gmail. UI and network work run
on separate threads. Recent pages are cached in SQLite; Gmail history
invalidates changed messages, including recovery when a history cursor expires.

## Build and demo

From the repository root:

```sh
bun install
bun run build --filter=@tjikos/gmailtjik
apps/gmailtjik/dist/gmailtjik --demo
```

For development: `cargo run --manifest-path apps/gmailtjik/Cargo.toml -- --demo`.
`--demo` uses synthetic messages and an in-memory cache. It does not contact
Google or Codex, access account credentials, or send mail. Demo Codex output is
explicitly a fixed synthetic proposal, not an LLM response.

Linux requires a working Wayland/X11 desktop, Vulkan-capable graphics, and GPUI's
native development libraries. On Fedora, these include `libxkbcommon-devel`,
`libxkbcommon-x11-devel`, `libxcb-devel`, `fontconfig-devel`, `freetype-devel`,
`wayland-devel`, and `vulkan-loader-devel`. Use the system `tijpset` font for the
established TjikOS typography; fontconfig provides fallback fonts.
The readable message body uses Inter (`rsms-inter-fonts` on Fedora) for distinct
regular, bold and italic faces.

## Connect Google

1. Create a Google Cloud project, enable **Gmail API**, and configure its OAuth
   consent screen for your account. Create a **Desktop app** OAuth client.
2. Keep the downloaded client JSON outside all Git repositories.
3. Optionally enable **People API** for saved contacts and Other Contacts.
4. Connect and open the app:

```sh
apps/gmailtjik/dist/gmailtjik --connect /private/path/google-client.json --contacts
apps/gmailtjik/dist/gmailtjik
```

Omit `--contacts` to connect only mail. The permissions are `https://mail.google.com/`, `gmail.settings.basic`, and,
when requested, `contacts.readonly` plus `contacts.other.readonly`. The browser
login uses a loopback callback, random state, PKCE, and refresh tokens. Public
distribution requires Google's applicable restricted-scope verification.
Google testing-mode refresh tokens can be short-lived; use the appropriate
OAuth publishing configuration for sustained personal use.

Private data lives in `$XDG_DATA_HOME/gmailtjik` (default
`~/.local/share/gmailtjik`). Override with an absolute `GMAILTJIK_DATA_DIR`.
Directories use mode 0700 and credential/cache files mode 0600. The app rejects
data directories inside a Git checkout. Each directory is bound to one account;
use separate directories for different accounts. The cache is not encrypted by
the application: use the machine's encrypted storage. Never put account files,
mail caches, credentials, captures, or Google client JSON in this repository.

## Search and contacts

Press Enter in search to query **all Gmail mail**, including messages absent
from the local cache. Gmail supplies conversation search through `threads.list?q=...`:
`from:person@example.com before:2024/01/01 has:attachment`. Use Older/Newer to
page through results. Gmail API search has documented differences from the web
UI, including alias expansion. Offline mode can show
previously cached pages; it is not a complete offline mailbox search index.

Recipient suggestions use the separate Google **People API**, including Other
Contacts, with its required warmup request. Suggestions are debounced and old
responses cannot replace a newer query. Type comma-separated recipients or
choose a suggestion. Suggestions work for To, Cc and Bcc. Contacts and Other
Contacts are imported into the private cache on connection and hourly thereafter;
Google search supplies suggestions when there is no local match. Suggestions group
addresses by Google's `resourceName` within the account-specific cache. Different
Google contacts stay separate even when their names match; entries without an ID
(including old cached entries until the next import) group only by email. A
single-address person selects immediately; multiple addresses expand into a sublist.
Up/Down and Enter also navigate and choose suggestions. Highlight
an address and press `d` (or click Set default) to toggle that person's default.
Defaults persist per account and sort first. Legacy defaults migrate to Google
IDs when their preferred email belongs to exactly one imported contact. To shows
an amber warning when you address that person without including their default address. This is advisory;
the app never substitutes a recipient or blocks sending because of a default.

People API contact/profile photos appear when Google provides one; otherwise the
person gets an initials avatar. Other Contacts search cannot return photos, so
those photos come from the full contacts import. Photos load and cache in memory
on an independent worker, without blocking mail synchronization or sending.

Conversation rows keep names and subject beside a compact circular avatar stack
on the right. Up to five distinct senders overlap slightly;
a +n circle lists remaining senders on hover. To/Cc-only recipients have no avatar. Aliases belonging to the same
saved Google person share an avatar and count once. Message headers retain the
individual circular sender avatar. Both prefer the cached Google contact
photo, then Gravatar (using the SHA-256 of the normalized email address). They
show a stable, locally generated identicon while loading or if no photo exists.
Avatar selection is independent of message display names. Conflicting Google
photos for the same address fall back to Gravatar, then a stable identicon.
List avatars are 28px circles with a 1px separator border and 8px overlap.
These lookups run separately from mail delivery; demo mode never fetches avatars.
Each expanded message can be collapsed with its header chevron. Recognized
quoted history, including Outlook reply-header chains, is hidden in both reading
views and can be restored with **Show quoted history**.

## Sync, notifications and keyboard

The default launch connects to real Gmail; only `--demo` uses fixtures. The
connection panel accepts a private Desktop OAuth client JSON path and opens
Google sign-in with Gmail and contacts permissions. The CLI connection command
remains available.

Email sync supports optional Gmail Pub/Sub push, with polling as a fallback. Install
the headless service to keep receiving mail and running rules with the GUI closed:

```sh
bun run build --filter=@tjikos/gmailtjik
apps/gmailtjik/scripts/install-sync.sh
systemctl --user status gmailtjik-sync@default.service
```

The user service runs the installed binary with `--sync`, uses the existing
private account/cache, and starts with the user manager. It does not need a
Wayland display. Like any user service, it stops when the user manager stops;
login lingering is an optional system policy, not enabled by the installer.
`gmailtjik --sync-once` performs one headless scan for diagnostics (stop the
account service first). A per-account lock prevents duplicate sync/notification
work across GUI instances and the service. Credentials are reloaded and refreshed
under a separate lock. The GUI reads the service's cached Inbox and can still
fetch selected messages, search, compose and send. Without a service, the GUI
owns polling while it is open.

The first scan establishes a silent baseline. Delivery receipts persist privately;
subsequently arriving unread mail is scanned across labels. Notjik sync queues
notifications durably for the active device; without configured sync, local
`notjikctl receive` is used. Gmail sync never marks mail read. Closing the GUI
does not stop an installed sync service.

Notifications prefer a matching cached Google contact photo, then the sender
domain's favicon, with the `gmailtjik` app icon as fallback. Favicon lookup sends
only the sender domain to Google's favicon service. Artwork is bounded to a
64-pixel PNG and cached privately for a day, including failed lookups; embedded
artwork travels with Notjik sync and renders in desktop Notjik.

Both email and authentication-code cards include **Open email**; code cards
also retain **Copy code**. Open email launches a new Gmailtjik window on the
originating desktop through Notjik's existing command-action routing, including
when invoked remotely. It selects the exact message and checks the account
before fetching the thread. It does not mark the email read. The native URI is
`gmailtjik://mail/open?account=…&message=…&thread=…`, accepted by
`gmailtjik --open URI`. Installing the updated desktop entry registers the
`gmailtjik:` scheme. This is a desktop app link, not an Android Gmail app link.

### Gmail push

Enable the Gmail API in the OAuth client project and connect the Gmail account
first. Then run the setup helper:

```sh
python3 apps/gmailtjik/scripts/setup-push.py --authorize
```

The helper opens a separate Google Cloud consent flow, resolves the Gmail OAuth
project, enables Pub/Sub, creates a topic, grants Gmail's publisher identity access,
and creates a dedicated pull subscription for this account instance. It preserves
existing topic IAM bindings. Cloud credentials and `push.json` stay in the private
account directory. Pass `--data-dir /private/account` for another account. Later
runs are idempotent and reuse the private Cloud sign-in. An existing authorized-user
credential can be supplied with `--credentials /private/cloud.json` instead.

**Settings → Sync** exposes topic, subscription, private credential path and an
enable switch. Credentials can be `authorized_user` JSON or a service-account key;
a service account needs Pub/Sub Subscriber on its subscription. The topic must
be in the **same project as the Gmail OAuth client**, and Gmail's
`gmail-api-push@system.gserviceaccount.com` needs Pub/Sub Publisher on the topic.
Give each account instance a separate pull subscription, even when sharing a topic.
No public HTTP endpoint is required. Google Cloud permissions and applicable
Pub/Sub billing apply; Gmail authorization alone cannot provision Cloud resources.

The background service loads changes automatically, renews the Gmail watch daily,
and treats Pub/Sub messages only as wake-up signals. It validates the account,
coalesces a batch into one Gmail history catch-up, and acknowledges matching
messages only after rules/notification delivery succeed. It never advances the
history cursor from a push payload or watch response. Failed batches redeliver;
persistent notification receipts prevent duplicate cards. Wrong-account or
malformed signals are discarded without processing mail.

Healthy push uses a five-minute recovery scan. If credentials, watch renewal or
Pub/Sub fail, the service returns to 30-second polling and retries push setup.
Disabling push stops this account's Gmail watch; Cloud resources remain available
for reuse. The Sync page shows connection/fallback status. A watch only wakes the
service; the computer and its user service still need to be running.

References: [Gmail push](https://developers.google.com/workspace/gmail/api/guides/push),
[Pub/Sub pull](https://cloud.google.com/pubsub/docs/pull).

### Rules

Open the **Settings** gear at the bottom of the sidebar. Settings replaces the
mail UI, with **Rules**, **Reading** and **Sync** in its sidebar. The Rules pane contains
ordered rules and an editor. Reading contains the remote-image preference.
Add, remove, enable/disable and reorder rules, then choose **Save rules**. Changes
apply on the next scan. Back to mail preserves unsaved edits during this session.
Rules belong to the account's private cache, not a particular email address.

Filters combine label name/ID, sender substring and subject substring; empty
filters match any value, and text matching ignores case. The first successful
handler wins. Handlers are compiled code, not arbitrary shell commands:

- **Extract authentication code**: local extraction and one Notjik code card with
  **Copy code** as its primary action. If extraction finds no code, try the next
  rule. The copy happens on the receiving desktop or Android device.
- **Email notification**: sender/subject notification.
- **No notification**: stop handling without displaying a card.

Defaults extract auth codes across all labels, then notify about other Inbox
mail. No matching rule means no notification. Existing mail before the scan
baseline is not replayed when settings change. The parser is deterministic and
local; mail is never uploaded for training.

### Separate accounts

Each instance has its own OAuth connection, private cache, rules, receipts and
service lock. Any authorized Gmail account can be connected:

```sh
GMAILTJIK_DATA_DIR=/private/path/work-mail gmailtjik --connect /private/oauth-client.json
apps/gmailtjik/scripts/install-sync.sh work /private/path/work-mail
GMAILTJIK_DATA_DIR=/private/path/work-mail gmailtjik
```

This enables `gmailtjik-sync@work.service`; no account address or credential is
stored in the unit or repository. The instance environment file contains only
the private data-directory path. Stop an account with
`systemctl --user disable --now gmailtjik-sync@work.service`.

Slash-separated Gmail labels form collapsible folders. Missing parent labels
appear as organizational holders; actual labels keep their Gmail identities.
Top-level folders receive stable palette colours, with lighter shades at deeper
levels. Click a chevron or use Enter/Space in sidebar navigation to fold a folder;
collapsed folders are remembered in the private cache. Colours follow the TjikOS
rainbow across root labels. Right-click a label or press `s` with it focused to
choose **Hidden**, **Visible**, or **Visible when new mail** (unread messages).
AUTH, Closed, Offers and Personal default to the unread mode. **Manage labels**
reveals hidden labels so these choices can be reversed. YELLOW_STAR/YELLOR_STAR
are suppressed because Starred already represents them.

The UI uses bundled Proicons. Outside text inputs, `r` replies to the selected
email and focuses the composer body, `a` archives it, and `d` moves it to Trash.
Delete also moves to Trash; permanent deletion requires the separate confirmed
Empty Spam or Empty Trash action. Existing drafts
are preserved, and replying again to the same message resumes its draft.

Press `?` outside an input for the shortcut guide. Escape returns focus to
navigation. Left/Right moves between labels, messages and reader; Up/Down selects
a label/message or scrolls the reader. Ctrl+F focuses search and Ctrl+N opens the
composer. Arrow keys inside text inputs keep their normal editing behavior.
Sending still requires the Send button and recipient review.

Startup shows the reader even when a local draft was recovered. Choose Compose
or Ctrl+N to resume that draft; Reply and applying a Codex draft also open the
composer. Close keeps the draft without reopening it on the next launch.

## Gmail snoozing

Gmail's public API does not expose the snooze-until timestamp. Gmailtjik leaves
the wake-up schedule with Gmail and adds `-in:snoozed` to Inbox queries,
Inbox Mark all read, and notification scans. When Gmail returns the mail to
Inbox, a subsequent sync can show it again. Other labels and explicit searches
remain unchanged; use `in:snoozed` in search to find snoozed mail.
This does not implement a local snooze scheduler or promise a wake-up notification
for old mail: notifications still use the existing arrival-time baseline.
The query construction is covered by synthetic HTTP tests; account-specific
Gmail search behavior requires live verification.

References: [Gmail message fields](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages),
[Gmail search operators](https://support.google.com/mail/answer/7190?hl=en).

## Codex

Install Codex CLI and sign in using `codex login`. The app launches
`codex app-server --listen stdio://` only when Ask is clicked or an instruction
is submitted. It sends the selected email and current draft to Codex. The
adapter initializes the protocol, creates an ephemeral thread, requests a
structured proposal, consumes streamed events, and supports cancellation.

This uses OpenAI's app-server protocol directly, not ACP. The protocol remains
experimental; update the adapter against the installed CLI schema as it evolves.
No model name is hardcoded; the user's Codex configuration selects it.

Codex can propose a draft, archive, star, or mark read/unread. The app shows the
proposal before applying it. Sending remains an explicit composer action with
recipient review. Agent output cannot request a send, shell command, or arbitrary
API call. Shell tools, apps, plugins, hooks, memories, web search and MCP servers
are disabled for the spawned server; filesystem access is restricted to a private
assistant directory. Email content is treated as untrusted quoted data.

## Reading and images

Ordinary messages open in a selectable **Readable view**. HTML is converted to
Markdown structure, preserving headings, paragraphs, lists, emphasis and safe
links while removing sender fonts, colours, backgrounds and empty spacing.
Simple wrapper tables and signature logos do not force the original layout.
Emphasis defined by embedded CSS classes is preserved as well as inline styling.
Quoted history remains collapsed until requested. Approved inline images remain
between their surrounding paragraphs, with their aspect ratio and colours intact.
Markdown resolves only already-approved image bytes; images still pass through
the existing image loader and remote-image setting. Inline images are not repeated
in the attachment list, and quoted images remain hidden with their quoted history.

In the message list, **Shift+Up/Down** extends or shrinks a contiguous selection.
The header shows the selected count; archive, delete and read/unread apply to those
conversations. Plain arrows, a normal row click, Escape or a view change clear the
range. Selecting a range does not automatically mark the previewed message read.

Recognized signatures are collapsed behind **Show signature** in both reading
views, including their logos and contact blocks. Detection uses explicit email
client signature markup, Gmail signature-image URLs, standard signature separators,
and short contact blocks after a standalone closing line. Ambiguous body text stays
visible. **Hide signature** collapses it again; signature and quoted-history
controls work independently, and the original message is retained.

Messages with data tables, multiple content columns or image-led layouts open in
**Original layout**, using Blitz HTML/CSS layout and CPU-rendered tiles, including
approved inline images. Each HTML message can switch between Readable view and
Original layout. Complex layouts default to dark surfaces with readable text,
links and borders. **Original colours** restores the sender's colours when needed;
image pixels remain unchanged in either mode. Layout detection is heuristic, and
the per-message switch provides a manual override.

Drag the inbox/reader divider to
resize the panes; the inbox width is saved privately, and HTML reflows after a
short debounce. Inline images fit the reading width without changing their
aspect ratio. Tables that cannot fit retain their full contents behind a
horizontal scrollbar. Message cards and collapsed previews keep their natural
height as the conversation scrolls. Rendering runs on a background thread with
a bounded cache.

Links open in the browser. Readable view supports selecting and copying text;
rasterized Original layout does not yet have text selection or full accessibility.
Scripts, forms and embeds are removed. Blitz has an offline resource provider:
CSS imports, fonts and image URLs cannot initiate network or filesystem access.
Only raster images approved by the existing image loader are supplied. Oversize
HTML (2 MB, 24,000 layout pixels tall, 4,800 pixels wide, or 48 million total
pixels) falls back to Readable view instead of truncation. Long emails are rendered in
consecutive tiles, including content extending below the document's main box.

The inbox and search list use Gmail thread pagination: one row per conversation,
grouped by Gmail's thread ID, with participant names, message counts, the original
subject and the latest snippet. Sent replies count toward the conversation;
any unread member marks its row unread. Archive, Trash, star and read/unread
toolbar actions affect the whole conversation. The reply target defaults to its
latest received message. Older cached message pages remain readable, with their
incompatible pagination cursors removed until the next successful refresh.

Folder navigation reads a private SQLite index of headers and label membership
first. It returns up to 25 conversations without scanning or decoding email
bodies; selection and scroll position are remembered separately for each view.
Saved lists and full conversation bodies open immediately. A fresh saved view is
reused for five minutes, and downloaded thread bodies for fifteen minutes;
Refresh explicitly bypasses folder freshness. Gmail history changes invalidate
folder views and affected threads, including newly added replies. A missing or stale
view refreshes in the background, with new previews appearing in batches of six.
Folder reads, selected-message reads, images, queued actions and background sync
have independent workers. Obsolete requests cannot replace the current view.

Successful preview batches are saved immediately, including when another request
in the same batch fails. They survive folder changes and app restarts without
replacing a completed saved view. When an exact first-page folder snapshot is
missing, known local messages can supply up to 25 cached conversations; arbitrary
searches and older-page cursors are not guessed. Failed refreshes keep the saved
rows visible and identify them as cached.

Recognized Google rate limits stop immediate retries. A per-account cooldown,
shared through the private database, honors Retry-After or defaults to 60 seconds.
Cached lists and already downloaded thread bodies remain available during it.
An already running background service adopts this behavior on its next restart.

Archive, Trash, star and read/unread actions update the list immediately after
their intent is saved in the private database. Pending intents are applied over
server snapshots, so refreshes and folder changes cannot undo them. Cached bodies
are retained, and older known conversations refill the first 25 rows while mail
is archived. Mark all read and confirmed Empty Spam/Trash also enter the queue;
arbitrary Gmail searches wait for the API to identify matching messages before
changing their read state locally.

The bottom-right activity counter shows pending changes. A per-account delivery
lock preserves FIFO order across the GUI and background service, and the queue
survives app restarts. Temporary connection/rate-limit failures back off and
retry. Permanent failures restore the affected local state while preserving
later intents, and remain in **Gmail activity** until dismissed or retried.
Partial bulk successes remain applied. Retry appends a new intent after any
later actions.

Confirmed sends are saved to the queue before the composer closes, so another
draft can be started immediately. Failed sends and draft saves retain their
contents behind **Review draft**. Once Gmail might have received a send, draft
creation or permanent deletion, an uncertain outcome is never replayed
automatically; check Gmail and review it again. Normal mailbox label changes are
idempotent and can retry safely. A running sync service must be restarted to
deliver queued changes while the GUI is closed; installation leaves it running.

Synthetic slow-delivery/rollback check:
`GMAILTJIK_DEMO_SCENE=optimistic bun run --cwd apps/gmailtjik test:ui`.

The reader fetches the complete Gmail thread, including sent replies, and shows
chronological message cards. Older messages can be expanded individually. Known
quoted reply sections are collapsed behind Show quoted history; inline answers
are preserved. Quote recognition is heuristic, and the original text is always
available. Per-message Reply retains Gmail threading headers.

Approved CID and remote raster images appear in their original positions in both
Readable view and Original layout. Unreferenced image attachments appear below
the body. Remote HTTPS images
load automatically for open messages by default. **Settings → Reading → Remote images**
switches between Auto and Ask, persisted per account. Ask provides a
per-message Load remote images button. Requests use no Google credentials or
cookies; redirects
and local-address hosts are rejected. Photos and logos keep their original
colours. A light image backing makes transparent dark logos readable, and a
per-view toggle lets you use a dark backing instead. No blanket inversion or
dimming filter is applied. Arbitrary SVG email attachments are not rendered.

## Checks and current boundaries

```sh
bun run check --filter=@tjikos/gmailtjik
cargo test --manifest-path apps/gmailtjik/Cargo.toml --locked
```

After building, `bun run --cwd apps/gmailtjik test:ui` captures the synthetic
inbox, composer, proposal, search, and retained draft on an isolated Wayland
output. It requires Hyprland, grim, wtype, a C compiler and wayland-scanner.
The HTML scene also uses Python Pillow to verify horizontal scrolling.
Captures stay under `/tmp`. `bun run --cwd apps/gmailtjik test:codex` explicitly
runs the otherwise ignored live Codex test using synthetic email only.

Live Gmail/People OAuth, sync, and sending need an authorized Google account;
synthetic tests do not establish those account-specific checks. A successful
local Codex handshake does not establish an authenticated model completion.
Sending is never automatically retried after possible dispatch; after an uncertain
response, check Sent before retrying. The first version has one recoverable local composer draft,
conversation inbox rows, and no attachment-upload UI or rich-text composer. Existing mailtjik archives and workers are independent.

References: [Gmail search](https://developers.google.com/workspace/gmail/api/guides/filtering),
[Gmail sync](https://developers.google.com/workspace/gmail/api/guides/sync),
[People contacts](https://developers.google.com/people/api/rest/v1/people/searchContacts),
[Other Contacts](https://developers.google.com/people/api/rest/v1/otherContacts/search),
[Codex app-server](https://developers.openai.com/codex/app-server),
[GPUI Kit](https://gpui-kit.com/docs/getting-started).

Synthetic HTML and divider check: `GMAILTJIK_DEMO_SCENE=html bun run --cwd apps/gmailtjik test:ui`.
Synthetic normalized reader check: `GMAILTJIK_DEMO_SCENE=reader bun run --cwd apps/gmailtjik test:ui`.
Synthetic inline-image check: `GMAILTJIK_DEMO_SCENE=inline bun run --cwd apps/gmailtjik test:ui`.
Synthetic signature check: `GMAILTJIK_DEMO_SCENE=signature bun run --cwd apps/gmailtjik test:ui`.
Synthetic progressive folder check: `GMAILTJIK_DEMO_SCENE=quick-folder bun run --cwd apps/gmailtjik test:ui`.
Synthetic failed-refresh cache check: `GMAILTJIK_DEMO_SCENE=cache-failure bun run --cwd apps/gmailtjik test:ui`.

Install the built app with `bash scripts/install.sh gmailtjik` from the workspace root. This installs `~/.local/bin/gmailtjik`, its desktop entry and the approved rose icon for launcher discovery.
If a headless sync binary is already installed, its copy is updated atomically too.
The running service is preserved and adopts the new build on its next restart.

Sidebar badges show Gmail unread message counts for each label. Right-click a label,
or choose Manage labels and its settings icon, to select Always show, Only when
unread, or Hidden. Manage labels also reveals hidden labels so these preferences
can be changed. Counts update after confirmed mailbox actions and during sync.

The message-list header offers an Unread toggle for the current label/search and
Mark all read for every matching unread message across all result pages. The
filter does not change the search text. Bulk changes remove only the UNREAD
label, preserve mail and drafts, and report failed or partially completed batches.

Spam and Trash offer **Empty Spam** / **Empty Trash** beside the folder title.
Clicking immediately permanently deletes every message in that folder across all
pages, regardless of the Unread toggle or search filter. Other folders have no
empty action.
Only successful deletions are removed from the cache; errors report partial
completion and preserve the remaining messages. Permanent deletion is not retried
automatically.

Existing connections with only `gmail.modify` must reconnect once in **Settings →
Sync → Gmail account**, using the private Desktop OAuth client JSON path. Google
requires the full-mail scope above for permanent deletion. The app explains the
missing permission before attempting to empty a folder; reconnecting does not
delete mail. To preview the controls without an account, use
`GMAILTJIK_DEMO_SCENE=empty-folders apps/gmailtjik/dist/gmailtjik --demo`.

Sender icons in the conversation list and reader open a contact-details modal.
It shows known addresses for the matching saved person, or the message's sender
name/address when no unambiguous contact is available. Copy buttons copy addresses;
Close and Escape dismiss the modal without changing message selection or expansion.

Gmail's public API has no unsubscribe method. Gmailtjik exposes valid HTTP(S) and
mailto links from a message's RFC 2369 `List-Unsubscribe` header in an Unsubscribe
helper. Opening a website or email application is an explicit action; it does not
mark the subscription as cancelled. RFC 8058 one-click POST delivery is not
implemented. Demo mode never opens these links. The synthetic `sender-details`
UI scene exercises both modal types without accessing real contacts or mail.

Idle sync checks Gmail history first and skips inbox and notification scans when
the last completed sync has the same cursor and a valid inbox snapshot (up to
fifteen minutes). Failed scans are retried rather than recorded as complete.
Background prefetch warms at most one folder per five minutes across processes,
and retains fresh prefetched folders for thirty minutes.

Message footer links and actions share a horizontal wrapping group below the body
and attachments, including reply, quoted-history and signature controls.

Expanded messages with loaded bodies are automatically marked read after the
viewer has been active for half a second. Summary-only and collapsed messages
are excluded. Read changes use the durable per-message action queue; unread-only
views keep the opened message in the reader instead of advancing through unread
mail. Explicit Mark unread is respected until the message is opened again.

## Gmail filters and reply drafts

Settings → Gmail filters shows mail **handlings**, grouped by the entire shared
action set: labels, archive/read/star/Trash/Spam behavior and forwarding. Open a
handling to edit its actions once and add or remove as many OR conditions as
needed. Each condition is a Gmail search; terms within a row combine normally,
and the rows are joined with OR. The reader's Filter button starts a new handling
with the sender prefilled.

The Apply labels field suggests existing names, including nested labels. Click a
match or use ↑/↓ and Enter/Tab; Escape dismisses suggestions. Earlier selections
are preserved and already selected names are omitted from suggestions.

Large handlings are split across bounded Gmail filter queries and still appear
as one handling. Saving creates and reads back all replacements before deleting
the originals. Full originals and progress receipts are journaled in the private
account cache; stale edits are rejected. Changing to an existing action set joins
its conditions. Existing advanced actions, including forwarding, are preserved;
unsupported criteria are shown without allowing a lossy edit.

“Also apply to existing matching mail” is off by default. Enabling it applies the
shared actions to matching existing mail, including Spam and Trash. Historical
forwarding is not supported. Deleting a handling changes filters only, leaving
previously processed mail unchanged. Accounts need `gmail.settings.basic`; use
Settings → Sync to reconnect if Google denies filter management.

For an explicitly requested account cleanup, `scripts/consolidate-filters.py`
provides separate `plan PRIVATE_DIRECTORY --since YYYY-MM-DD` and
`apply PRIVATE_DIRECTORY` modes. Planning saves original filters and per-query
received-mail activity evidence outside Git. Non-delete address conditions with
no received mail since the cutoff and no-action filters are removed from the
plan; delete conditions and non-address conditions are preserved. Sender
activity is checked independently of obsolete subject text, including Spam and
Trash and excluding Sent and Drafts. Applying verifies replacements before
removing originals, records every mutation, and verifies the final inventory;
it never applies actions to existing messages. Interrupted creates require
receipt reconciliation; mail sync and archive workers keep running. Run it with
`uv run apps/gmailtjik/scripts/consolidate-filters.py ...` to use its declared
Python dependencies. Settings deletions are serialized over a pooled connection;
slow responses are reconciled by filter-ID read-back and every completed ID is
recorded immediately.

Synthetic checks: `GMAILTJIK_DEMO_SCENE=handling-groups bun run --cwd apps/gmailtjik test:ui`
and `python3 -B apps/gmailtjik/tests/test_filter_consolidation.py`.

Replying while a different draft is open saves that draft locally and opens the
reply. Resume saved drafts from the composer; their text and pending Gmail-save
receipts survive reopening the app.

Signature folding also recognizes a trailing block whose full name and email
match the sender and which includes a phone number. Name-only matches are not
enough. Postscripts remain visible; Show signature restores the complete block.

HTML footer links load HTTPS `/favicon.ico` icons in a separate background worker.
Icons are cached by origin in the private data directory's `favicons/` folder for
seven days; failed lookups are cached for one day. PNG, JPEG, GIF, WebP and ICO
images are bounded and reduced to 32px. Missing or unsupported icons keep the
standard link icon. Demo mode uses synthetic icons and makes no favicon requests.

The expanded message header groups sender and To/Cc/Bcc icons with address
tooltips, Original layout and signature icon toggles, a relative timestamp (hover
for the exact date), Reply, and Reply All when there are multiple other recipients.
Reply All excludes the current account, deduplicates addresses, and never copies Bcc.
Check with `GMAILTJIK_DEMO_SCENE=message-header bun run --cwd apps/gmailtjik test:ui`.

Selecting a label fills search with `label:<full name>` (quoted when needed).
Keep that prefix while adding terms to search only the selected label; Gmail's
label ID filter remains active even with OR queries. Replacing/removing the
prefix permits a global search. The virtual All view has no label prefix.
