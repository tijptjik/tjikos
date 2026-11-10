# GTK idle CPU profiling, 2026-11-10

The initial whole-service idle measurement was 8.4% of one CPU core. That was excessive;
relative improvement over Quickshell was not a sufficient acceptance criterion.

## Identified work and changes

- Auto-hide called animation wake functions every 25 ms while visible, scheduling
  redraws even for static icons and text. Wake only on reveal; real state changes still
  start their animations, and truncated text retains its ticker.
- Static and unmapped icons no longer register frame callbacks. Mapping resumes real
  morph/dot/pulse animations without retaining the widget through its own signal.
- Unchanged recording helper heartbeats and provider poll counters triggered full bar
  reconstruction. Ignore identical activity and diagnostic-only changes while retaining
  updated backend state, command acknowledgements, errors and real content updates.
- Audio and network spawned multiple status helpers at one/three-second intervals.
  `pactl subscribe` and adapter-owned `nmcli monitor` now wake their providers on
  changes. A 30-second audit reconciles missed events; subscription failure restores the
  existing polling cadence, retries every two seconds and wakes immediately after
  reconnect. Explicit configured polling intervals are respected. Subscription tasks and
  children are owned by their provider lifecycle.
- Default Notjik status now uses its existing bounded native IPC client. Custom control
  commands remain supported. GTK Fcitx status uses bounded, non-activating D-Bus reads;
  the original command transport remains the compatibility fallback.
- Hypridle inspection reads the user manager's unit metadata/files directly instead of
  repeatedly invoking `systemctl cat` and `pgrep` for the managed active service. Native
  and legacy live status were compared exactly; unsupported/unavailable cases retain the
  existing command path. No service policy or action ordering changes.
- The backend publish timer sleeps when no update is pending; D-Bus subscriptions
  request only the relevant interfaces instead of all session signals.
- The D-Bus decoder now checks the variant type before unwrapping it. Previously normal
  values triggered GLib critical diagnostics; private-bus tests now make these fatal.
- Configuration identifier/dimension and monitor resolution regexes now compile once.
- Codex weekly usage ranked all historical samples at every refresh. A correlated
  latest-row seek uses the existing account/time index. The original and new query
  returned identical rows in the same read-only live transaction: 396.93 ms versus 0.21
  ms of CPU. No database indexes, schema or account data were changed.

## Instrumentation

`BARTJIK_PROFILE_DIR` enables per-process JSON CPU counters in an existing directory.
Synchronous spans use thread CPU time; async provider spans measure each future poll
rather than wall time across suspension. Nested spans overlap and must not be summed.
With the variable absent, no profiling timer or output writer is created.

Use whole-service cgroup `cpu.stat` deltas for the total, including short-lived helpers.
Per-process `/proc` CPU and proportional memory identify contributors, but omit exited
helpers when sampled only at interval endpoints. CPU is percent of one core, not total
machine capacity. Keep all panels closed and record bar visibility/output topology.

Intermediate 30-second service samples were 8.37%, 6.83% after render fixes, and 4.96%
after audio/network subscriptions and native Notjik IPC. These guided profiling; display
topology changed during this session, so they are not a controlled before/after claim.
The renderer component dropped from approximately 2.0% to 0.5–0.6% in those samples.

## Validation

GTK unit tests: 42 passed, four explicitly gated UI fixtures excluded. Tests cover
presentation changes versus poll bookkeeping, subscription events and disconnection.
Private-bus services cover Fcitx reads plus media/tray owner changes, actions and menus.
Private audio/network transport fixtures, 13 idle-policy tests, native process-lifecycle
fixtures and six monitor regression tests passed. Synthetic SQL fixtures cover legacy
window classification, explicit weekly windows, timestamp ties, inactive accounts and
missing usage. Build/static/assets checks passed.

Artifacts are ephemeral under `/tmp/bartjik-*-check.log`,
`/tmp/bartjik-idle-profile-*.json`, and `/tmp/bartjik-quota-benchmark.json`.

For a reproducible whole-service sample without restarting or changing visibility:

```sh
dev-check python3 apps/bartjik/scripts/measure_process_tree.py \
  --unit bartjik.service --seconds 30 --output /tmp/bartjik-idle.json
```

Run repeated windows to include slower providers. Confirm panels are closed and record
concealment before/after; an open panel or running animation is a different workload.

## Installed measurements and limits

Clean 30-second windows with profiling disabled, two bars on DP-2 and no open or
detached panels measured 1.94% CPU / 85.80 MiB median PSS concealed, and 3.17% CPU /
84.99 MiB median PSS revealed. These totals include the service backend and helpers,
including CPU consumed by helpers that exited during the sample. Static icons reported
zero active tick callbacks before and after the revealed sample. A warmed-up repeat
measured 3.31% CPU / 86.86 MiB PSS revealed, again with no active icon callbacks.

The original 8.4% visible / 9.6% concealed measurements used three bars across two
outputs. The topology changed during investigation; these are not matched measurements
and must not be presented as an exact percentage improvement. The remaining cost is
nonzero: provider reconciliation, pointer sampling for auto-hide, state updates and
rendering continue. No animations or interaction features were disabled to manufacture
an idle result. This pass does not establish active-preview performance or a new matched
Quickshell comparison.

Latest validation: supported build passed, 42 GTK tests passed (four gated fixtures
ignored), and the private-bus services fixture passed with fatal GLib criticals enabled.
Final sample artifacts: `/tmp/bartjik-icon-hidden.json`, `/tmp/bartjik-icon-visible.log`
and `/tmp/bartjik-cpu-final-visible.json`.

## Follow-up: visible idle work

The hover timer rebuilt every panel's target, cloned provider rows/styles and queried
widget geometry at 40 Hz even with the pointer outside the bar. The closed-panel path
now checks bar bounds first. Pending/open panels and detached windows retain the full
tracking path. The cheap path still steps the deck to clear dismissal suppression;
entering a bar uses the existing cadence and opening delay.

The backend also serialized the full desktop snapshot for every provider poll counter
and timestamp. It now batches those two diagnostic fields on the existing one-second
reload cadence. Provider content, health, errors, unknown extension fields and actions
retain immediate publication through the existing 30 ms coalescer. Diagnostics are still
updated in memory immediately and flushed with the next real update or audit.

A same-session, 30-second visible profile before/after measured 3.36% versus 1.64% total
service CPU. GTK accounted for 1.67% versus 0.43%; the backend for 1.20% versus 0.83%.
Full snapshot publication fell from about 4.6 to 1.7 calls/second. These instrumented
samples include profiling overhead; clean follow-up measurements are recorded below.

Validation: supported build, 42 GTK tests, one new backend diagnostic classification
test and private-bus service fixtures passed. The GTK suite includes hover-gap
corridors, pending-panel cancellation, pinning and dismissal/re-entry suppression. Four
gated renderer fixtures were not run. No real pointer movement or device mutation was
used for these checks.

With profiling disabled, the successful clean visible sample measured **1.69% CPU** and
**108.88 MiB median PSS**, with three bars (DP-1 status/workspace and DP-2 workspace),
no open/detached panels and zero ticking icons at both endpoints. Hidden idle measured
**1.64% CPU / 107.86 MiB PSS**. The earlier two-bar samples used a different topology;
these values must not be converted into an exact percentage improvement or memory
regression. One visible attempt was discarded because a bar concealed during sampling.
The instrumented before/after is same-session evidence, not a controlled topology or
provider-state benchmark.

Artifacts: `/tmp/bartjik-visible-profile-next.json`,
`/tmp/bartjik-visible-profile-after.json`, `/tmp/bartjik-visible-clean-next.json`,
`/tmp/bartjik-hidden-clean-next.json`, and `/tmp/bartjik-visible-idle-check.log`.
