# Native runtime and installation

Runtjik ships `runtjik-gtk`, `runtjik-core`, native assets and the page-preview helper.
GTK4 owns windows and controls; the separate Rust backend uses GTK3/GdkPixbuf for icon
resolution. The shared task view does not own task storage or policy.

Use `dev-check bun run build --filter=@tjikos/runtjik`, validate, then install with
`scripts/install.sh`. Managed hosts use `TJIKUP_MANAGED_CONFIG=1`. The installer stops
only the resident launcher, replaces payloads atomically and preserves recording/browser
workers on their existing executable inode. Configuration, credentials and histories
stay outside the repository and are not reset.

`runtjik show`, `close`, `quit` and `status` control the GTK instance. `runtjik --demo`
uses synthetic data; tests must establish workspace-7 placement before launch. See
[acceptance coverage](acceptance.md) and [desktop adapters](desktop-adapter.md).
