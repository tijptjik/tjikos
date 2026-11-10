# Runtjik desktop adapter

Runtjik's Rust core and GTK frontend use `packages/tjik-desktop`. Bartjik has not been
changed. The package contains toolkit-independent models and focused `DesktopSession` /
`WindowActions` contracts, plus statically linked Hyprland and deterministic fake
implementations. GTK surface mechanics remain in `tjik-gtk`.

## Selection and diagnostics

Run `runtjik desktop-status`. Set `TJIK_COMPOSITOR=hyprland` or `TJIK_COMPOSITOR=fake`
before starting Runtjik to override automatic selection. Quit/reopen an existing
frontend to change its selection. Ordinary IPC outages and compositor restarts on the
selected runtime/display recover automatically. Automatic selection requires an
unambiguous Hyprland hint and a successful read-only version handshake. Unknown choices
and competing session hints report unavailable rather than guessing. Fake selection
opens no compositor endpoint; it is a desktop test double, not a sandbox for launching
applications or other desktop services. Use `--demo` and private runtime/data/bus
directories for interactive fixtures.

The adapter pins the runtime and Wayland display, and validates each connection by
socket inode and compositor identity. The lock's Wayland display must match
`WAYLAND_DISPLAY`; nested sessions cannot silently fall through to the host socket.
Worker processes inherit the compositor selection and the currently resolved endpoint
identity, including service-manager launches. On reconnection the adapter scans only
that runtime for a unique live endpoint whose lock identifies the same Wayland display.
It never searches another display or falls through to a host session. Diagnostics report
adapter, version, capabilities and errors without publishing endpoint paths or window
titles.

## Migrated paths

| Previous owner                                                        | Current boundary                                                                                     | Validation                                                  |
| --------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| Launcher window matching and switch                                   | Normalized `Window` serialization, opaque ID activation                                              | Core fake test, synthetic GTK window picker                 |
| `windows.rs` focus and special workspace handoff                      | Hyprland adapter owns monitor/reveal/focus ordering and scrolling-column revelation                  | Adapter focus-order and injection tests                     |
| `close.rs`, browser window close/focus                                | Adapter activation/close with fresh target lookup; process identity and pidfd policy stay in Runtjik | Core close tests, browser regressions                       |
| Screenshot/window/workspace targeting and recording workspace polling | Normalized windows, output/workspace identities and logical geometry                                 | Capture regressions; recording lifecycle remains in Runtjik |
| Recording stop shortcut                                               | Optional adapter operation with lifetime-bound cleanup                                               | Source boundary check; actual recording was not started     |
| Image viewer placement                                                | Normalized output association and optional placement                                                 | Size unit test; no personal image opened                    |
| GTK monitor placement, outside click and cross-output drag            | Worker queries and normalized output geometry; optional adapter observer                             | Private compositor GTK interaction suite                    |
| Direct pointer socket reads on GTK thread                             | Bounded worker queries and asynchronous GTK callbacks guarded by gesture generation                  | Private compositor drag/transfer suite                      |

The UI wire request field named `address` is retained for compatibility, but its
contents are opaque window IDs. Production IDs contain a session generation and cannot
be passed to another session. Controllers do not parse these IDs. The fake adapter
deliberately uses non-address IDs and records mutations without accessing the host.

## Contract and limits

Calls are synchronous and object-safe, intended for existing core/GTK workers. No
runtime-specific future/stream or GTK object crosses the boundary. GTK never performs
compositor IPC on its main thread.

`DesktopSession::subscribe()` returns a toolkit-independent blocking subscription.
`DesktopEvent` carries a monotonically increasing sequence, a `resync` flag, and either
a complete normalized snapshot or an unavailable error. Initial subscriptions,
reconnects, and coalesced updates require full-state replacement. Each subscriber has
one pending state; a slow consumer receives the latest state with `resync=true`, rather
than accumulating an unbounded queue or applying an incomplete delta sequence. Snapshot
revisions use the same sequence. Late subscribers get the current state atomically with
registration; dropping the source wakes receivers. Fake adapters publish through the
same contract.

The Hyprland implementation attaches its event socket before its initial snapshot.
Events trigger coalesced reconciliation; a five-second read-only audit also catches
silent missed updates. Reconnect uses bounded backoff from 100 ms to two seconds, opens
a new event stream, and publishes a fresh resync snapshot. A replacement compositor has
a new generation: previous window/workspace/output identities remain stale even if
native addresses are reused. Actions fail while unavailable and are never queued for
replay. Only process-owned passive observer/shortcut registrations are restored.
Snapshot queries are separate compositor reads, not a compositor-side atomic
transaction.

Runtjik GTK refreshes visible launcher counts and window pickers from desktop events,
without replacing the search text or triggering auto-launch/auto-selection. Updates wait
for in-flight work and IME composition. Hidden views do not refresh from these
notifications. A disappearing last window leaves an empty picker; observation cannot
fall through into launching the application. Other provider/index refresh timers retain
their existing responsibilities.

A compositor restart destroys GTK's Wayland connection. The `serve` process therefore
supervises a separate internal `surface` process and retains launcher state in memory.
After the selected display returns it relaunches GTK, restores the search, pin and view
state, and keeps previously hidden windows unmapped. An explicit quit, including during
downtime, cancels recovery. Unrelated crashes do not cause an unconditional restart
loop. Backend children exit with their GTK parent; independently launched
recording/browser workers are not killed. No GUI actions or pending mutations are
replayed. Transient task-editor forms are not reconstructed; this is launcher-session
recovery, not a new draft-storage system.

During an outage, `runtjik status` reports `recovering:true`; normal GTK status includes
`desktop_sequence`. `runtjik desktop-status` continues to report selected adapter,
capability and error information. Changing to a different runtime or Wayland display is
a new session selection and requires a new launch; recovery deliberately does not guess
across that boundary.

The Rust source dependency check covers application/controller/widget code. Hyprland
transport belongs in `tjik-desktop`; widgets consume normalized models and capabilities.
Niri and broader capture transport negotiation remain follow-up adapter work.
Desktop-service and recording/file/history policy remain in their existing owners.

## Validation and recovery

- `dev-check cargo test --manifest-path packages/tjik-desktop/Cargo.toml --offline`:
  seven tests initially passed, including synthetic IPC, rotation/scaling, stale
  generation, rejected acknowledgement and timeout.
- `dev-check cargo test --manifest-path apps/runtjik/Cargo.toml --offline`: 31 tests
  passed.
- `dev-check bun run build --filter=@tjikos/runtjik`: GTK/core release build and browser
  extension validation passed.
- `python3 -B apps/runtjik/tests/desktop_boundary.py`: rejects direct compositor
  commands, socket names, environment detection and Lua dispatch in Rust application
  sources.
- Live read-only handshake/diagnostics verified Hyprland 0.57.1, fake and unknown
  selections.

The first GTK regression run exposed a demo fixture conversion mismatch between query
and switch; normalize fixtures at their source rather than adapting only one controller
path. The initial pointer-cache design also failed the incremental drag regression;
asynchronous callbacks now apply fresh positions with gesture-generation guards. The
subsequent private compositor suite passed incremental dragging, mixed-scale
cross-output transfer, outside-click focus, keyboard input and both frontend paths.

Installation uses the existing `scripts/install.sh`, with `TJIKUP_MANAGED_CONFIG=1` to
preserve managed configuration and avoid installing unrelated Tasktjik artifacts. Binary
replacement is atomic; existing browser/recorder workers retain their old inode until
naturally restarted. A backup of the prior installed tree is retained outside the
repository before replacement. Reopen Runtjik to use the new build; do not kill active
capture workers to pick up an update.

### Adapter-seam validation, 2026-11-09 (before event recovery)

- Adapter tests: 7 passed; Runtjik Rust unit tests: 31 passed.
- `dev-check dbus-run-session -- python3 -B apps/runtjik/tests/desktop_adapter.py`:
  actual private-window discovery, activation, close, stale target and display mismatch
  passed. Probe build:
  `dev-check cargo build --manifest-path packages/tjik-desktop/Cargo.toml --release --locked --offline --example probe`.
- `dev-check dbus-run-session -- python3 -B apps/runtjik/tests/gtk_ui.py`: passed,
  including incremental drag, cross-output transfer, outside click, keyboard control and
  legacy comparison. Synthetic renders: `/tmp/bc-4m0ui8ww/gtk-launcher.png`,
  `gtk-transfer.png`, `gtk-narrow.png`.
- `dev-check dbus-run-session -- python3 -B apps/runtjik/tests/gtk_desktop.py`: passed
  IME preedit/commit/cancel, Unicode, 1.5x/2x logical dimensions, startup and comparison
  isolation. Artifacts: `/tmp/bc-7hlo7br8/`.
- Remaining package regressions all passed: private compositor cleanup, command
  transitions, reading, migration, clipboard, capture clipboard/history, stats, browser
  extension/reconnect/offline history.
  `dev-check bun run check --filter=@tjikos/runtjik` passed.
- The complete package test command stops at the existing frozen `rust_parity.py`
  fixture mismatch. Both the pre-update installed binary and new build produce the same
  five differences: `auto_launch_applications`, `auto_launch_commands`,
  `close_on_outside_click`, and indexed/total counts 266 versus fixture 264. A direct
  old/new replay of all 53 protocol cases passed. The old frozen fixtures were not
  rewritten as incidental work.
- Validation logs: `/tmp/runtjik-adapter-validation.log`,
  `/tmp/runtjik-adapter-final-checks.log`, `/tmp/runtjik-adapter-remaining.log`,
  `/tmp/runtjik-adapter-baseline-parity.log`, `/tmp/runtjik-adapter-parity.log`.
- Desktop IPC read/write deadlines are bounded: one second for normal queries, 75 ms for
  pointer queries. Cold private-compositor initialization demonstrated why pointer and
  discovery deadlines must be separate.

Adapter-seam release SHA-256 (superseded by the event/recovery build):

```text
0a6836ec63452bd63250ecb2990b6253aa819371da5200052dccaedbfcc82aee  runtjik-core
29d63752cb66461c8d0919acc526fab9488a1d2a5508933146d4ce8bed015aed  runtjik-gtk
```

Next architecture task: use this event/recovery contract for the Bartjik adapter slice,
retaining Runtjik regression coverage.

### Push events and recovery follow-up

- `dev-check bun run --filter @tjikos/runtjik test:desktop` builds/tests the shared
  adapter and runs private native-window, pushed-UI and restart tests. All visible
  fixture parents are placed on workspace 7 silently before launch, with private D-Bus
  sessions.
- Ten adapter tests cover ordered/coalesced delivery, resubscription, teardown, changed
  snapshots, disconnect/restart, stale targets and no action replay.
- `desktop_push_ui.py` sets the normal refresh interval to 300 seconds and verifies that
  native window open/close updates counts within four seconds, preserves the query, and
  never executes the synthetic launch command.
- `desktop_recovery.py` restarts an actual private compositor twice while the same
  adapter watcher survives: visible GTK restores query/pin state; hidden GTK stays
  unmapped; old backend children terminate; quit during a third outage prevents
  relaunch.
- `desktop_observe.py` verifies that zero, one and multiple matching windows never
  auto-select or launch during observer refresh, including fake IDs. It is also part of
  the normal package test command.
- Recovery artifacts from the first complete regression pass: `/tmp/bc-zqjphll3/`;
  IME/comparison artifacts: `/tmp/bc-eb781p8q/`. Logs:
  `/tmp/runtjik-events-focused.log`, `/tmp/runtjik-events-regression.log`.

Final event/recovery validation: `test:desktop` passed all ten shared adapter tests plus
native actions, pushed GTK counts and visible/hidden compositor restarts. Core unit
tests (31), observer tests, browser/capture/reading/clipboard/stats regressions, static
checks and all 53 old/new protocol cases passed. The existing frozen fixture mismatch
remains documented above. Final restart artifacts: `/tmp/bc-mjxc0jav/`. Final logs:
`/tmp/runtjik-events-final.log`, `/tmp/runtjik-events-core-checks.log`. Installation and
rollback receipt: `/tmp/runtjik-events-install.json`.

The exact installed event/recovery binary hashes and rollback directory are recorded in
`/tmp/runtjik-events-install.json`.

The installed launcher exposed a quit-handshake race during replacement: the GUI could
exit before its I/O thread acknowledged quit. Quit now acknowledges before scheduling
termination; repeated quit/status requests tolerate a confirmed closed listener while
preserving errors from a still-live listener. The pushed-UI and restart tests also cover
repeated quit and offline quit.

Final frozen-package verification: concurrent scrolling edits required an isolated
source snapshot at `/tmp/runtjik-events-frozen-ufrb3sjh`. Its release build,
observer/push/restart tests, GTK UI/IME/mixed-scale tests, 31 core unit tests, 53-case
old/new protocol replay, and remaining browser/capture/task-storage regressions passed.
The snapshot includes a repository-root marker so synthetic `/tmp` task data is
correctly outside its repository. Logs: `/tmp/runtjik-events-frozen-final.log`,
`/tmp/runtjik-events-frozen-core.log`, `/tmp/runtjik-events-frozen-remaining.log`;
restart artifacts: `/tmp/bc-1i0y1xvp`; IME artifacts: `/tmp/bc-h3nfqz3g`. The supported
installer uses this frozen package; hashes and rollback information are in
`/tmp/runtjik-events-install.json`.
