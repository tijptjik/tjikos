# Install the Runtjik Browser Bridge

The bridge connects Runtjik to native Linux Zen or Firefox. It requires Firefox 142+ or
a compatible Zen release, plus a separate Runtjik installation. Window focusing uses
Hyprland. Flatpak/Snap integration, Android, macOS and Windows are not supported by this
installation guide.

Version 0.3.0 passed Mozilla's automated screening and is publicly available as of
2026-11-03. Mozilla may review it further. Install the signed release from the listing
below; a local unsigned ZIP cannot serve as the permanent installation.

## Install

1. Build and install Runtjik using the repository's normal installation workflow. Its
   installer also registers the native messaging host. Existing installations can repair
   registration with:

   ```sh
   bash ~/.local/lib/runtjik/browser-extension/install-host.sh
   ```

2. Open the
   [Runtjik Browser Bridge listing](https://addons.mozilla.org/en-GB/firefox/addon/runtjik/)
   **in the Zen/Firefox profile you normally use** and install it. Accept the
   browsing-activity permissions. Alternatively, install the Mozilla-signed XPI through
   `about:addons` → gear → **Install Add-on From File**. An end user does not need AMO
   signing credentials. Remove a temporary development copy if it masks the signed
   installation; keep browser signature enforcement enabled.
3. In `~/.config/runtjik/runtjik.conf`, add `"Browser"` to the existing
   `[plugins].enabled` array, preserving your other providers. Reopen Runtjik.
4. Visit a normal HTTP(S) page, then try `/tabs`, `/browser` and `/history`. Private
   windows are excluded, and pinned tabs are omitted from tab lists.
5. Fully quit and restart Zen/Firefox. Confirm the bridge remains enabled in
   `about:addons`, then repeat those commands. No temporary reload, debugging port,
   browser startup script or separate bridge service should be needed.

The extension starts its native host when the browser starts and retries a disconnected
host after five seconds. Refresh the Runtjik query after reconnecting. Each connection
gets a new session ID; refresh any selection made before a restart.

## History when Zen is closed

In the intended browser profile, open `about:profiles` and copy its **Root Directory**,
which contains `places.sqlite` (not its Local Directory/cache). Merge this section into
your existing configuration; do not duplicate a TOML section that is already present:

```toml
[plugin_settings.Browser]
history_profiles = ["/home/YOU/.zen/YOUR-PROFILE"]
history_browser = ["/usr/bin/zen-browser"]
```

Use an absolute profile path, replacing the example with your actual directory. For
native Firefox, use its profile and executable instead. One executable serves all
configured offline profiles, so do not mix Firefox and Zen profiles. Reopen Runtjik
after changing settings.

Quit the browser completely and search `/history`. Results should be marked `offline`.
Search matches saved titles and URLs as literal substrings, with ASCII case folding; `%`
and `_` are literal characters, not wildcards. Matches are ranked by last visit, newest
first, with at most 100 results across configured profiles. Visit count is displayed but
does not change that ordering. The 100-result cap is applied after matching, so a search
can find an older visit absent from the initial list. Unicode text can match exactly;
non-ASCII case folding is not provided.

Enter rechecks that the visit still exists and launches a new tab in that exact profile.
Ctrl+Enter cannot delete offline history. Once any bridge connects, live browser history
takes over on the next query; offline profiles are not merged in, even if the live query
has no matches. Disconnecting all bridges restores offline access. No browser daemon,
hidden window or copied history index is required. Remove `history_profiles` to disable
offline access.

## Troubleshooting

| Symptom                                               | Check or action                                                                                                                                                                                                                                                                                                   |
| ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Listing unavailable                                   | Use the current `/runtjik/` listing linked above and check its availability on AMO. Do not install the unsigned build ZIP or disable signature checks.                                                                                                                                                            |
| Extension disappears after restarting                 | Check `about:addons` for a signed installation in the same profile. Loading through `about:debugging` is temporary.                                                                                                                                                                                               |
| “Browser bridge disconnected”                         | Start the correct profile, enable the bridge, wait at least five seconds, and refresh the query. Repair host registration if necessary, then restart the browser.                                                                                                                                                 |
| Browser reports missing native host                   | Check `~/.mozilla/native-messaging-hosts/org.tjikos.runtjik_browser.json` and that its `path` names an executable installed wrapper. The native host name is `org.tjikos.runtjik_browser`. Custom browser builds may need `install-host.sh --manifest-dir PATH`; Flatpak/Snap need integration not provided here. |
| “Reload the updated bridge and allow history access”  | Check the installed extension version and accept its history permission in `about:addons`. Reload the extension or restart the browser, then refresh `/history`.                                                                                                                                                  |
| “Cannot read configured browser history”              | Check the absolute Root Directory, that `places.sqlite` exists, and that your user can read it. Do not change database permissions globally or copy a live profile into the repository.                                                                                                                           |
| “Browser: database is locked”                         | The offline reader cannot access the configured profile database while another process holds a conflicting lock. With Zen/Firefox open, enable the signed bridge and refresh the query so history uses the browser API. For offline access, quit the browser completely.                                          |
| History is empty                                      | Visit a normal HTTP(S) page in the intended profile. Private visits, hidden entries and unvisited bookmarks do not appear offline. Any connected bridge makes live history authoritative.                                                                                                                         |
| History order/search differs with browser open        | Offline search uses SQLite substring matching and last-visit ordering; live matching is provided by Firefox/Zen's history API.                                                                                                                                                                                    |
| Old selection fails after restart or history deletion | Refresh `/history` or `/tabs` and select again. Stale results must not act on reused browser IDs.                                                                                                                                                                                                                 |
| Tab opens but focusing fails                          | Confirm Runtjik and the native browser are in the same Hyprland session. Tab opening and compositor focus are separate operations.                                                                                                                                                                                |

## Release acceptance

For each signed release, use a normal native browser profile and record the browser and
extension versions alongside these results:

- Install the signed release, quit the entire browser, restart, and verify `/tabs`,
  `/browser`, and `/history` without a temporary reload.
- Quit again; verify offline search and opening a result into the intended profile. Once
  the bridge connects, refresh and verify the `offline` label goes away.
- Disable/re-enable the bridge in `about:addons`; verify offline fallback while disabled
  and live results after reconnection. Refresh old selections.
- With two profiles connected, close one and confirm the other still supplies live
  results; close both and confirm configured offline history returns.

Synthetic regression tests cover the launcher, real native-host process/socket lifecycle
and extension retry logic. They do not establish signed-install persistence, real
browser permissions or Hyprland focus; those require the checks above. See the
[release guide](../browser-release/README.md) for test commands.
