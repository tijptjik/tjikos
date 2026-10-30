# Runtjik Browser bridge (development preview)

Native Linux Firefox and Zen can expose their windows/tabs through this opt-in
extension. No debugging port, HTTP server, separate systemd service, or browser
profile scraping is needed. Firefox starts one native host per extension
connection; each host owns a private Unix socket in
`$XDG_RUNTIME_DIR/runtjik-browser`. Multiple browser profiles can coexist.

## Setup

1. Build and install Runtjik using its normal install scripts.
2. Run `python3 ~/.local/lib/runtjik/browser-extension/install-host.py`.
   This registers the host in `~/.mozilla/native-messaging-hosts`, verified with
   the native Zen installation on this machine. Do not infer this directory
   from the profile location (`~/.zen`) or packaging wrapper: they can differ.
   If your browser build uses a different native-host directory, pass
   `--manifest-dir PATH`.
   Flatpak/Snap sandbox integration is not implemented.
3. In Zen/Firefox, open `about:debugging#/runtime/this-firefox`, choose **Load
   Temporary Add-on**, and select this directory's `manifest.json`. Temporary
   extensions disappear on browser restart. Permanent installation requires a
   Mozilla-signed XPI; signing/distribution is not included in this preview.
4. Add `"Browser"` to your existing `[plugins].enabled` array in
   `~/.config/runtjik/runtjik.conf` and reopen Runtjik. Preserve other providers.

```toml
[plugin_settings.Browser]
columns = 3 # 1–6
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

The extension needs `tabs` and `nativeMessaging` permissions. Private windows
and pinned tabs are excluded, even if the extension is allowed in private
browsing. Titles, URLs, favicon URLs and audio/unloaded status are returned on
demand. Page content and screenshots are not sent. Favicons use embedded image
data or HTTPS URLs loaded by the launcher without browser cookies; unsupported
icons fall back to the browser icon or title initial. Zen remote-image wrappers
are unwrapped. Browsing metadata stays in memory, outside indexes and bridge
logs, unless you explicitly save a reading item. The
launcher may retain its last results while hidden. Existing command-history
settings apply to filter text you type; disable `[history].enabled` if desired.
Socket permissions restrict access to your user, not to a particular same-user
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
