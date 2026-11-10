# Page preview helper

Toolkit-independent Python helper for Runtjik and Tasktjik link previews. It renders
isolated browser previews, falls back to site metadata, and caches results outside the
repository. Hosts own process lifetime, cancellation and user-facing policy.

`page_preview.py --batch` reads an array of URLs on stdin. The existing cache and
process contracts are unchanged by extraction from Tasktjik. Runtjik bundles a regular
copy at `helpers/page_preview.py`. Tasktjik retains its existing installation path
through a source symlink; its build dereferences that link so installed files are
self-contained.

Run `dev-check bun run --filter @tjikos/page-preview test` for synthetic cache tests. No
GUI or network is required by those tests.

The helper retains its original GPL-3.0-or-later license; see `LICENSE` and `NOTICE`.
