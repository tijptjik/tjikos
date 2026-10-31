# Reviewer notes

Runtjik Browser Bridge is a companion for the Runtjik Linux desktop launcher,
part of https://github.com/tijptjik/tjikos. The add-on contains plain JavaScript
with no transpilation, minification, downloaded code, dependencies or build
step. The package contains only manifest.json, background.js and icon.png.

The native application is installed separately. On native Linux Firefox/Zen,
build the repository with Bun and Rust, install Runtjik following its README,
and run its browser-extension/install-host.sh if registration needs repair.
The host name is org.tjikos.runtjik_browser and its allowed extension is
runtjik-browser@tjikos.local. It uses stdio native messaging and private
per-session Unix sockets. It does not expose a TCP listener. Full desktop
window focusing currently requires Hyprland; history queries do not.

Enable Browser in Runtjik's plugins.enabled array. Use synthetic HTTP(S)
visits and tabs to test /tabs, /browser, and /history. Enter activates a tab or
opens a history item. Ctrl+Enter closes a tab or deletes all visits for the
selected history URL. A window close goes through the native host's Hyprland
integration, allowing the browser's normal confirmation UI. Shift+Enter does
not delete history. Private windows are excluded and pinned tabs are excluded
from tab listings. History is queried only for /history; general launcher
search requests tabs, not history. Results are bounded to 100 history entries.

The required browsingActivity data declaration covers the metadata sent to
the local application. See browser-extension/PRIVACY.md for full disclosure,
including native favicon fetching and optional launcher command history.
There is no extension telemetry or remote data service. The extension uses
the browser installation consent on Firefox 142+; older versions are excluded.

The native application has a separate, explicitly configured read-only
history fallback when no extension session is connected. This reads existing
Places databases without copying/indexing or modifying them. It is independent
of the extension and is disabled by default. No accounts are required for the
browser functionality. Tasktjik account setup is unrelated and unnecessary.
