# Espanso on Wayland

Espanso 2.4.0 can remain stuck at `Querying modifier status...` when its
`Espanso.SyncTool` window does not receive keyboard focus. A dictionary edit can then
produce `could not restart worker, as the exit process has timed out`. The service may
still report running even though no worker is ready to expand text.

The [modifier-sync patch](../examples/espanso/patches/wayland-sync-timeout.patch) bounds
this optional synchronization to 500 ms and handles the sync window closing. If no
modifier state arrives, startup continues without that initial snapshot. Normal key
event detection and injection still initialize afterward.

The patch targets Espanso revision `a87d1723f47a59e38d370e2f0c636d66d568b8f6` (version
2.4.0). In a separate checkout at that revision:

```sh
git apply /path/to/tjikos/apps/runtjik/examples/espanso/patches/wayland-sync-timeout.patch
dev-check cargo build --release --locked --no-default-features --features modulo,vendored-tls,wayland -p espanso
```

Keep a rollback copy before replacing the locally installed Espanso binary. Preserve its
existing input-device access configuration: the repaired installation used
`cap_dac_override=p`, applied to the replacement with explicit user approval. Restart
`espanso.service` after replacement and verify that the worker reaches
`using WaylandFallbackClipboard`; `espanso status` alone does not prove readiness. A
future upstream binary replacement must include this fix or an equivalent upstream
change.

For regression verification, place `Espanso.SyncTool` silently on workspace 7 with focus
disabled **before** restarting. Add and then remove a temporary match file containing
only `matches: []`, waiting for a fresh ready worker after each change. Remove the
temporary placement rule afterward. Do not inject test text into the user's active
window or copy their dictionary into the repository.

The local repair was built and installed with the existing capability. Both real
config-change reloads completed in 2.3 seconds while the sync window was denied focus,
and the temporary match file was removed. The installed binary matched the build.
Physical typing remains a separate end-to-end check.
