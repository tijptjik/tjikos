# Native bundled runtime migration

All six bundled providers now have Rust implementations. The Tasktjik adapter
shares recurrence and task mutation rules with Tasktjik's native API. Clipboard,
Screenshots and Recorder no longer require the Python compatibility host. The
Firefox native-messaging host, recorder worker, desktop Trash/undo and imv
lightbox helper also run in Rust; the countdown uses Quickshell/QML.

Tasktjik preserves dedicated-config precedence, stable device identity under a
file lock, mutation UUIDs, local socket HTTP, validated Tailscale HTTPS URLs,
proxy/redirect rejection and exact-peer DNS fallback with TLS hostname validation.
Local JSON stores keep their format, per-task atomic writes, directory fsync and
file lock. Local/demo mutations now use the same validation and revision rules as
the native Tasktjik API. Vault plans exclude links escaping the vault.

Recorder IPC remains compatible with existing workers. The worker owns its own
encoder and stops only that child, including on workspace changes or shutdown.
Capture paths remain outside the repository; history skips symlinks and active
recordings. Clipboard previews use at most two background jobs and private
session directories; recording thumbnails likewise use two background jobs.
Trash undo verifies the original path and refuses to overwrite an existing file.

Python is intentionally retained for the reference backend, third-party API-v1
extensions and build/test/registration scripts. Legacy SSH authorities still run
the remote Python Tasktjik service; the local adapter is Rust. The separate
Tasktjik notification CLI is outside this Runtjik migration. Enabling third-party
extensions can still start the compatibility host. No live service was replaced,
no installation or push was performed, and unrelated working-tree changes were
preserved. Existing Firefox host registrations need to be registered again after
installation to select `runtjik-core browser-host`.

## Validation

- Workspace `bun install --frozen-lockfile`, `bun run build` and `bun run check`.
- 129 Python reference tests, 48 executable parity checks, 13 Rust tests and
  12 native migration integration tests; Tasktjik's 10 native tests also pass.
- Synthetic HTTP/socket authority fixtures cover headers, device identity,
  mutation UUIDs, dedicated configuration, DNS fallback, redirect rejection,
  local JSON interoperability, recurrence and vault containment.
- Bundled demo query/activation parity and no compatibility-host child checks;
  native-messaging framing, stale reply IDs and socket cleanup.
- A fake encoder verifies recorder readiness, status, stop, private output and
  socket cleanup without recording any desktop.
- Real QML in a private compositor covers Todo/Reading, Clipboard, capture menus
  and galleries, rendering, warm reopen and concurrent cold opens. Countdown
  completion and Escape cancellation pass. Native imv sizing and targeted close
  pass using a generated synthetic image and the established floating-window rule.
- A uniquely named synthetic file passes desktop Trash and undo with content
  preserved. This check uses the home filesystem and the desktop Trash service;
  GIO rejects Trash on `/tmp`'s internal tmpfs mount.

Personal browser tabs, remote authority availability, real recording encoders and
portal interaction were not exercised. The browser and recorder protocols were
verified with synthetic peers; they are not claims of live service acceptance.

## Measurements (2026-10-30)

Fifteen alternating Python/Rust trials, using the same synthetic index and a
private compositor, after builds and tests completed. Cold means a fresh process
with retained OS/icon caches, not a filesystem-cache flush. Lookup includes JSON
pipe serialization. UI opening measures layer/input readiness, not first painted
pixels. Memory is sampled PSS/RSS summed across UI/backend descendants, excluding
the compositor; RSS double-counts shared pages.

| Measurement | Python reference | Rust |
| --- | ---: | ---: |
| Backend cold start | 109.83 ms | 16.11 ms |
| Backend warm resume | 26.27 ms | 2.48 ms |
| Lookup | 2.29 ms | 1.20 ms |
| UI cold open | 370.52 ms | 261.13 ms |
| UI warm show | 84.23 ms | 92.34 ms |
| UI warm toggle | 46.72 ms | 46.61 ms |
| Backend PSS / RSS | 40.7 / 65.0 MiB | 14.9 / 26.4 MiB |
| Open UI + backend PSS / RSS | 140.8 / 254.4 MiB | 115.3 / 214.0 MiB |
| Hidden UI + backend PSS / RSS | 132.9 / 245.8 MiB | 108.5 / 206.5 MiB |

Backend cold start, warm resume and resident memory improve substantially. Cold
UI opening also improves. Warm `show` is slower in this sample; `toggle` is close
to the reference. Quickshell/IPC still dominates those warm UI operations, so
these results do not support claiming a consistent warm-UI speedup.

[Raw samples, distributions, process memory and binary hash](native-runtime-2026-10-30.json).
The raw `baseline_commit` records the starting main commit; the binary hash
identifies the measured working-tree build for this migration. Live recording or
network-provider performance was not benchmarked.

## Reproduce

```sh
bun run build
bun run check
(cd apps/runtjik && bun run test)
cargo test --manifest-path apps/runtjik/Cargo.toml --release --locked
cargo test --manifest-path apps/tasktjik/Cargo.toml --release --locked
python3 -B apps/runtjik/tests/rust_ui.py
python3 -B apps/runtjik/tests/rust_helpers_ui.py
python3 -B apps/runtjik/scripts/benchmark_rust.py /tmp/native-runtime.json --ui --trials 15
```

UI checks require access to Wayland to create their private compositor. Helper
checks move only a synthetic file through desktop Trash and restore it. All
fixtures, images, logs, indexes and task data remain outside the repository.
