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

Omit `--contacts` to connect only mail. The permissions are `https://mail.google.com/` and,
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

HTML mail uses Blitz for HTML/CSS layout and CPU-rendered tiles in the GPUI reader,
including tables and approved inline images. Drag the inbox/reader divider to
resize the panes; the inbox width is saved privately, and HTML reflows after a
short debounce. Rendering runs on a background thread with a bounded cache.

HTML links open in the browser. Use **Text · select and copy** for selectable
text; rasterized HTML does not yet have text selection or full accessibility.
Light/dark email backing can be switched without inverting image pixels. Author
colours are preserved, so unusually styled messages may need light backing.
Scripts, forms and embeds are removed. Blitz has an offline resource provider:
CSS imports, fonts and image URLs cannot initiate network or filesystem access.
Only raster images approved by the existing image loader are supplied. Oversize
HTML (2 MB or 24,000 layout pixels tall) offers Text view instead of truncation.

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
reused for 60 seconds; Refresh explicitly bypasses freshness. A missing or stale
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

Approved CID and remote raster images appear in their HTML positions. Plain-text views show image attachments below the body. Remote HTTPS images
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
Synthetic progressive folder check: `GMAILTJIK_DEMO_SCENE=quick-folder bun run --cwd apps/gmailtjik test:ui`.
Synthetic failed-refresh cache check: `GMAILTJIK_DEMO_SCENE=cache-failure bun run --cwd apps/gmailtjik test:ui`.

Install the built app with `bash scripts/install.sh gmailtjik` from the workspace root. This installs `~/.local/bin/gmailtjik`, its desktop entry and the approved rose icon for launcher discovery.

Sidebar badges show Gmail unread message counts for each label. Right-click a label,
or choose Manage labels and its settings icon, to select Always show, Only when
unread, or Hidden. Manage labels also reveals hidden labels so these preferences
can be changed. Counts update after confirmed mailbox actions and during sync.

The message-list header offers an Unread toggle for the current label/search and
Mark all read for every matching unread message across all result pages. The
filter does not change the search text. Bulk changes remove only the UNREAD
label, preserve mail and drafts, and report failed or partially completed batches.

Spam and Trash also offer **Empty Spam** / **Empty Trash**. Confirming **Delete
permanently** deletes every message in that folder across all pages, regardless
of the Unread toggle or search filter. Cancel or Escape leaves the folder alone;
changing folders dismisses the confirmation. Other folders have no empty action.
Only confirmed deletions are removed from the cache; errors report partial
completion and preserve the remaining messages. Permanent deletion is not retried
automatically.

Existing connections with only `gmail.modify` must reconnect once in **Settings →
Sync → Gmail account**, using the private Desktop OAuth client JSON path. Google
requires the full-mail scope above for permanent deletion. The app explains the
missing permission before attempting to empty a folder; reconnecting does not
delete mail. To preview the controls without an account, use
`GMAILTJIK_DEMO_SCENE=empty-folders apps/gmailtjik/dist/gmailtjik --demo`.
