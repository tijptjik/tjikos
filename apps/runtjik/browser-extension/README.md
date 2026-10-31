# Runtjik Browser bridge

Native Linux Firefox and Zen can expose their windows/tabs through this opt-in
extension. No debugging port, HTTP server, or separate systemd service is
needed. Firefox starts one native host per extension
connection; each host owns a private Unix socket in
`$XDG_RUNTIME_DIR/runtjik-browser`. Multiple browser profiles can coexist.

## Setup

1. Build and install Runtjik using its normal install scripts.
2. The installer registers the host automatically. To repair registration, run
   `bash ~/.local/lib/runtjik/browser-extension/install-host.sh`.
   The host lives in `~/.mozilla/native-messaging-hosts`, verified with
   the native Zen installation on this machine. Do not infer this directory
   from the profile location (`~/.zen`) or packaging wrapper: they can differ.
   If your browser build uses a different native-host directory, pass
   `--manifest-dir PATH`.
   Flatpak/Snap sandbox integration is not implemented.
3. Install the Mozilla-signed release XPI using `about:addons` → gear →
   **Install Add-on From File**, or use its AMO listing once published. Accept
   the browsing-activity permissions. It loads automatically on every browser
   start. Requires Firefox 142+ or a compatible Zen version. The ZIP produced
   by the local build is unsigned and is not a permanent-install artifact.
   Signing/publication requires the maintainer's AMO account; see
   [release instructions](../browser-release/README.md). Do not disable browser
   signature checks. Temporary loading is for extension development only.
4. Add `"Browser"` to your existing `[plugins].enabled` array in
   `~/.config/runtjik/runtjik.conf` and reopen Runtjik. Preserve other providers.

```toml
[plugin_settings.Browser]
columns = 3 # 1–6
# Optional: search history even when the browser is completely closed.
# Use absolute paths from about:profiles; no automatic profile discovery.
history_profiles = ["/home/YOU/.zen/YOUR-PROFILE"]
history_browser = ["zen-browser"] # use ["firefox"] for Firefox profiles
```

`/browser` (alias `/zen`) shows window cards. `/tabs` shows tabs in a list.
Append text to filter titles, case insensitively. Window search also matches its
contained tab titles. Enter focuses the window and activates the selected tab. Shift+Enter on a
window opens only its unpinned tabs in Runtjik; typing filters that list, and
Escape returns to general search. Leaving `/tabs` clears the window filter. Ctrl+Enter closes
the selected tab through Zen, or the selected desktop window through Hyprland.
Window closure first identifies the exact client using a temporary unique title
marker, then sends a graceful compositor close request. Zen may show its own
confirmation dialog. Closing a window closes its contained tabs,
including pinned tabs. Shift+Enter currently reports that Read later integration
is pending in the separate Tasktjik app; it does not save or close anything. Cards currently show browser icons, active-tab titles
and tab counts, **not screenshot thumbnails**. Both connected browsers appear
in these commands; `/zen` does not restrict the results to Zen.

The bridge activates the tab through the extension, temporarily prefixes the
window title with a unique marker, finds that exact Hyprland client, and uses
Runtjik's existing workspace-aware focus operation. It restores the previous
prefix afterward, with an extension-side fallback timer. This avoids ambiguous
title matching when different windows display the same page. A title marker may
briefly be visible in window decorations or bars. Zen-specific workspace/space
switching beyond the standard Firefox tabs API still needs live verification.

The extension needs `tabs`, `history` and `nativeMessaging` permissions. Private windows
and pinned tabs are excluded, even if the extension is allowed in private
browsing. Titles, URLs, favicon URLs and audio/unloaded status are returned on
demand. Page content and screenshots are not sent. Favicons use embedded image
data or HTTPS URLs loaded by the launcher without browser cookies; unsupported
icons fall back to the browser icon or title initial. Zen remote-image wrappers
are unwrapped. Browsing metadata stays in memory, outside indexes and bridge
logs, unless you explicitly save a reading item. The
launcher may retain its last results while hidden. Existing command-history
settings apply to filter text you type; disable `[history].enabled` if desired.
See [privacy details](PRIVACY.md), including native offline history and favicon
requests. Socket permissions restrict access to your user, not to a particular same-user
process. Requests and native messages have size/time bounds. Browser restarts
create new session IDs so old selections cannot target reused browser IDs.

No live-browser compatibility claim is implied by unit tests. Test duplicate
page titles, multiple windows/profiles, minimized/special-workspace windows,
Zen spaces, tab moves/closes, browser reconnects, and private-window exclusion
before relying on this as your daily window switcher.

## Reading integration (deferred)

Reading-list storage and filing are moving to the separate Tasktjik app. The
previous embedded filing action is disconnected while that integration is built.
No remote authority deployment is part of these browser changes.

General search includes matching unpinned tabs and carries their favicons, just
like `/tabs`. Browser window cards use the active tab's favicon when available.
The general results heading is hidden unless it conveys a specific mode or pin
status. Reload the extension in about:debugging after updating its files.


## History search

`/history` shows up to 100 recent HTTP(S) history entries across connected
profiles. `/history words` searches the browser's saved titles and URLs, across
its retained history. Results are ordered by last visit and show the URL and
visit count. Enter opens a new tab in the same profile and focuses its normal
window. Private windows are never used. Ctrl+Enter removes all visits for the selected URL from that profile’s
history and refreshes the current search. Shift+Enter leaves history unchanged. An entry removed since the query cannot be reopened through this action.

This uses the browser history API on demand. It does not scrape profiles or
copy history into Runtjik's persistent index, and history is not mixed into
general search. The existing launcher command-history preference still applies
to typed search terms. Firefox/Zen do not record private-browsing visits in the
history API. All connected profiles are searched, as with the tabs integration.

After updating from bridge 0.1.0, reload the extension in Zen's
`about:debugging#/runtime/this-firefox` and allow its new history permission.
Reloading also reconnects the native host so it recognizes the new operations.
When no extension session is connected, `/history` can read the explicitly
configured `history_profiles` directly. This is an on-demand, read-only SQLite
query, including committed WAL changes, with no database copy or persistent
index. HTTP(S) visits only are returned, ordered newest first and capped at 100.
Offline text matching is SQLite's case-insensitive substring search (ASCII case
folding); it may differ from Firefox's history search. Results are marked
`offline`. Enter revalidates the visit and launches `history_browser` with the
exact profile and URL. Ctrl+Enter refuses deletion until the extension connects
and the search is refreshed. One configured browser executable serves all
offline profiles; do not mix Firefox and Zen profiles in that list.

Closing or quitting Zen is supported: offline search needs no browser process,
hidden window, autostart daemon, or headless copy of the profile. Active tab and
window control still requires a running browser and connected extension. When
any bridge session is connected, live history is authoritative; offline profiles
are not merged into that response. Remove `history_profiles` to disable offline
access independently of the browser extension.
