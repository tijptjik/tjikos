# Page preview helper

Toolkit-independent Rust helper for Runtjik and Tasktjik link previews. It renders
isolated headless Chromium previews, falls back to site metadata and favicons, and
caches results outside the repository. Hosts own process lifetime, cancellation and
user-facing policy. No Python interpreter is required at runtime.

`page-preview --batch` reads a JSON array of URLs on stdin and emits one JSON object per
line, flushing cached results before fetching uncached pages. Without `--batch`, stdin
contains one URL. Existing version-4 receipts and images under
`$XDG_CACHE_HOME/tasktjik/page-previews` remain usable. Images persist until removed;
image-free results retry after five minutes.

HTTP requests have bounded sizes and deadlines. Each browser render uses an empty
profile and a 25-second deadline. SIGTERM or SIGINT cancels the request; the browser
process group is killed and reaped before its temporary profile is removed. Cancellation
during HTTP work completes within the request's five-second timeout.

Both applications bundle the executable: Runtjik under `helpers/page-preview`, and
Tasktjik under `qml/page-preview` for both its GTK and retained Quickshell views. The
build scripts produce self-contained installed helper files.

Run `dev-check bun run --filter @tjikos/page-preview test` for Rust cache tests and
synthetic HTTP/browser integration tests. Tests use private temporary state, a loopback
server and fake browser processes; they do not access user accounts or launch windows.
Python is used only as a development test driver.

The helper is licensed under GPL-3.0-or-later; see `LICENSE` and `NOTICE`.
