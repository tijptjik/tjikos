# Native migration: bundled providers and runtime helpers

Historical migration report: the later [native-only release](native-only.md)
removes the Python compatibility paths described below.

The initial Rust core was merged into main in `3ca928e` (implementation commit
`7f6fb90`). The native core now also owns all bundled providers and runtime helpers.
Tasktjik's adapter uses the Tasktjik Rust model for local/demo mutations and
recurrence; it preserves dedicated-config precedence, device identity, request
UUIDs, HTTPS Tailscale validation and DNS fallback, socket and legacy SSH modes.
Nothing was installed into the live desktop or pushed remotely.

## Current boundary

| Path | Implementation |
| --- | --- |
| Controller, discovery, icons, search/ranking, pagination, history, launch/focus | Rust, existing QML UI |
| Command menu, shortcuts, general command search, stats | Rust |
| Config browsing, validation, comment-preserving edits and explicit migration | Rust |
| Graceful application close, persistent retry tracking, SIGTERM/SIGKILL escalation | Rust |
| Words: dictionary/thesaurus lookup, independent source results and activation | Rust, HTTPS transport through curl |
| Browser: window/tab lists, filters, scoped navigation, focus/close requests | Rust, existing Unix-socket protocol |
| Clipboard, Screenshots, Recorder, Tasktjik adapter | Rust |
| Third-party API-v1 extensions | Python compatibility host and reference query routing |
| Firefox native-messaging bridge, recorder worker, Trash/undo, image viewer | Rust |
| Recording countdown | Quickshell/QML |

Python remains for the explicit reference backend, third-party API-v1 extensions,
and build/test/registration scripts. Legacy SSH Tasktjik authorities retain their
remote Python command until those authorities are upgraded; HTTPS/socket/local
modes require no Python adapter. The Tasktjik application notification CLI itself
is outside this Runtjik migration. Existing browser host registrations need to be
registered again after installation to point at `runtjik-core browser-host`.

The following timings and validation describe the earlier migration stage;
see `native-runtime.md` for the completed bundled runtime migration.

## Behavior retained

Configuration edits use a file lock and preserve surrounding TOML comments. Values
are validated before publication, existing file modes are retained, and an external
editor's intervening change is detected before atomic replacement. Migration takes
a private backup, fills only missing defaults, and is idempotent. It does not run
implicitly during installation.

Close retries retain the SQLite request ledger. A retry is bound to the process
owner, boot identity and process start time, with a second identity check after
opening a pidfd. Signals go through that pidfd, so PID reuse cannot redirect a close
retry. Unrelated terminal commands do not inherit GUI close targets. Window-specific
close requests still target only the selected window. Unsaved-work confirmation
remains the application's responsibility on the first graceful request.

Words uses at most three network worker threads. A new query invalidates queued
older work; stale responses cannot replace current results. Completed sources are
published while other sources remain pending. Curl runs with its user config
disabled, HTTPS-only initial/redirect protocols, response-size bounds and timeouts.
Tests deliberately queue the slow source first to verify independent publication.

Browser retains the private runtime-directory check, bounded responses, socket
timeouts, exact session/window/tab identifiers, pinned-tab exclusion, favicon
validation and tab-scope reset when leaving `/tabs`. The extension's separate
Python native-messaging bridge was unchanged in that earlier stage. It is now
ported as described in `native-runtime.md`; no installed connection was replaced.

## Measurements

Same benchmark harness and 15 alternating Python/Rust trials as the initial report:
1,014 indexed entries (1,001 synthetic fixtures and 13 existing Flatpak exports),
isolated state, populated application/icon caches and retained OS caches. UI tests
use a private nested Hyprland compositor. No personal application is activated by
the benchmark. “Cold” means fresh processes, not reboot-cold/empty-cache startup.

| Median latency | Python reference on current main | Current Rust |
| --- | ---: | ---: |
| Full UI cold `show` | 385.39 ms | 271.31 ms |
| Resident synchronous `show` | 88.35 ms | 94.87 ms |
| Resident hotkey `toggle` until ready | 48.45 ms | 47.24 ms |
| Backend cold first response | 112.48 ms | 16.49 ms |
| Backend `resume` | 26.20 ms | 2.53 ms |
| Lookup round-trip | 2.31 ms | 1.21 ms |
| First `/define` prompt | 0.13 ms | 0.05 ms |

Warm UI timings vary; this run does not support a warm synchronous-opening speedup.
The cold UI and lookup gains remain substantial. The `/define` measurement opens
an empty prompt and performs no network request. The first-stage Rust implementation
needed about 102 ms to start Python for it; this native path no longer starts that
host.

| PSS snapshot | Python | Rust |
| --- | ---: | ---: |
| Backend after normal queries | 39.31 MiB | 11.65 MiB |
| Backend after `/define` | 38.99 MiB | 11.65 MiB |
| Full launcher open | 120.12 MiB | 95.01 MiB |
| Full launcher hidden | 112.51 MiB | 88.63 MiB |

These sum root and descendant processes, excluding the compositor/harness. UI
memory settles for one second; backend memory is sampled after the final requests.
They are individual snapshots, not median memory values. Small Python before/after
changes reflect shared-page accounting, not proof that opening a prompt frees
memory. Other providers still using the compatibility host retain its memory cost.

[Raw samples, PSS/RSS process breakdowns and binary/source hashes](native-providers-2026-10-30.json).
The legacy raw key `with_provider_memory` now means “after `/define`”; it does not
mean that the Python compatibility host was started. Its process list confirms only
`runtjik-core` for Rust. The raw baseline commit identifies the merged base; the
binary hash identifies this continuation's measured executable.

## Validation

- Full workspace `bun run build` and `bun run check` pass (unaffected packages can
  use Turbo cache). `bun install --frozen-lockfile` required no dependency changes.
- All 129 existing Python unit tests pass with local socket access.
- 48 executable Python/Rust protocol comparisons pass, plus startup/resume,
  shared-history and no-Python-child assertions.
- Nine native tests pass, including live synthetic-process pidfd identity checks,
  config comment preservation, invalid-value rejection, dictionary parsers and
  browser scope behavior.
- Five migration integration tests pass: native menus/config/close without a
  Python host, config save/backup/migration modes and idempotence, Browser parity,
  real private Unix-socket framing and targeted actions, and asynchronous dictionary
  source/stale-result behavior with a synthetic curl transport.
- Real QML checks pass: searchable `/todo`, ordinary/empty searches, dictionary
  results, compatibility-provider results, rendering, same-PID warm reopen, and
  concurrent cold opens. The synthetic screenshot was visually inspected.
- A separate private-compositor test passes native window selection, exact-window
  focus, targeted graceful close and closing-state cleanup against two synthetic
  Kitty clients. Close confirmation is disabled only for those fixtures. This
  avoids the older keyboard-injection smoke test's environment dependency.

The dictionary test uses synthetic HTTP responses; public dictionary service
availability was not an acceptance gate. Browser actions are tested against the
existing protocol with a private socket fixture, not a live personal browser.
No archive worker or unrelated dirty source files were modified by this migration.

## Reproduce

```sh
bun run build
bun run check
(cd apps/runtjik && bun run test)
cargo test --manifest-path apps/runtjik/Cargo.toml --release --locked
python3 -B apps/runtjik/tests/rust_ui.py
python3 -B apps/runtjik/tests/rust_close_ui.py
python3 -B apps/runtjik/scripts/benchmark_rust.py /tmp/runtjik-native-providers.json --ui --trials 15
```

Socket tests need local Unix-socket access; UI checks need access to the current
Wayland session to create their private compositor. Recordings/screenshots use
only synthetic `--demo` data. Runtime fixtures, history, logs and images remain
outside the repository.
