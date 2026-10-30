# Rust core migration and paired measurements

The native core cuts process-cold launcher startup by 30% in this workload,
reduces normal-search helper PSS by 72%, and reduces median lookup round-trip
latency by 46%. Warm hotkey reopening is effectively unchanged. Quickshell/QML
continues to account for most full-application memory and warm opening time.

This is an experimental native core migration, **not a Python-free replacement**.
It is isolated on branch `runtjik-rust`; it has not been installed into the live
desktop or merged into main.

## Implementation boundary

Rust owns the short-lived controller and its per-display lock, the resident
JSON-line server, configuration reads/validation, Gio application discovery,
Fish-history command filtering, cache invalidation, icon lookup/rasterization,
fuzzy ranking, stable ordering and pagination, SQLite history/frecency,
application dispatch, and Hyprland window selection/focusing. The existing QML
layout and JSON protocol remain in use. Native icon handling calls GTK 3 and
GdkPixbuf, retaining their theme and SVG behavior.

An on-demand Python compatibility process retains API-v1 providers (including
Words, clipboard, captures, Browser and the Tasktjik adapter), command menus and
configuration editing/migration, and graceful-close/escalation behavior. Normal
application search, launch, window selection and focus do not start it. Third-party
providers that contribute general-search results require the host immediately.
The host remains resident once used, retaining provider state and asynchronous
jobs. `runtjik quit` terminates both the native helper and its host.

The unchanged Python backend remains selectable with `RUNTJIK_BACKEND=python`.
Quit before changing backend; a resident instance otherwise continues running
its original backend. SQLite history retains its schema and private location.
Rust index/icon manifests use separate filenames so comparisons do not overwrite
the reference backend's cache format. All runtime data remains outside the repo.

## Measurements

Measured 2026-10-30. Each startup/reopen measurement has 15 trials, with alternating
Python/Rust order. Both backends index the same 1,014 entries: 1,001 synthetic
application fixtures plus 13 installed Flatpak exports discovered by both indexers.
No exported application was activated. The workload uses a stock Adwaita icon
theme, default providers, empty isolated history, and a page size of 100 (30 for
lookup timing). There are 120 timed lookups per backend across eight queries.

“Cold” means a fresh process with populated application/icon caches and retained
OS filesystem caches; this is not a reboot-cold or empty-cache benchmark. Backend
timing ends on its first complete JSON response. UI timing ends at layer/input
readiness in a private nested Hyprland compositor, not first painted pixels on a
physical display. Hotkey timing includes polling until ready, not just submission
of the IPC call. UI measurements use real discovery over synthetic desktop files;
separate visual checks use only `--demo` fixtures.

| Latency, median | Python | Rust | Change |
| --- | ---: | ---: | ---: |
| Full UI, cold `show` | 378.90 ms | 265.61 ms | 29.9% lower |
| Resident synchronous `show` | 89.00 ms | 75.82 ms | 14.8% lower |
| Resident hotkey `toggle` until ready | 50.47 ms | 49.50 ms | 1.9% lower |
| Backend cold, first result | 113.77 ms | 14.23 ms | 87.5% lower |
| Backend `resume` | 25.68 ms | 2.47 ms | 90.4% lower |
| Lookup JSON round-trip | 2.16 ms | 1.17 ms | 45.8% lower |
| Lookup p95 | 2.77 ms | 1.62 ms | 41.5% lower |

The 1 ms warm-toggle difference is too small relative to the trial ranges to claim
an improvement. Warm `show` has overlapping ranges too; this is a local measured
median, not a general latency guarantee. Full cold-start ranges were 364.57–383.05 ms
for Python and 252.98–269.90 ms for Rust.

| Memory | Python PSS / summed RSS | Rust PSS / summed RSS |
| --- | ---: | ---: |
| Backend after normal lookups | 38.62 / 64.82 MiB | 10.71 / 24.48 MiB |
| Full launcher open | 124.95 / 252.70 MiB | 98.05 / 211.46 MiB |
| Full launcher hidden | 114.99 / 243.02 MiB | 90.23 / 202.87 MiB |
| Backend after first compatibility-provider use | 38.75 / 64.97 MiB | 43.60 / 83.66 MiB |

Memory sums the relevant process root and all descendants, excluding the benchmark
harness and compositor. UI samples settle for one second; backend samples are taken
after the final trial's requests. These are individual snapshots, not median memory
samples. PSS apportions shared pages; summed RSS double-counts shared mappings.

The compatibility cost is material: first `/define` use takes **101.86 ms** with
Rust versus **0.10 ms** with the already loaded Python backend. This is an empty
word-lookup prompt, not an internet request. Afterwards, the combined Rust/Python
helpers consume about **4.85 MiB more PSS** than the reference helper. Thus the
current memory benefit applies to the native path, not every session. Porting the
remaining bundled providers/actions or releasing their host safely on dismissal
is follow-up work, not a benefit claimed by these measurements.

[All raw samples and per-process memory](rust-benchmark-2026-10-30.json) include
binary/source hashes and the baseline commit. The baseline Python file is unchanged
from that commit. Timing and memory should be remeasured on the actual configured
index before making a fleet-wide migration decision.

## Validation

- Focused Bun build and syntax/format/contract checks pass.
- 127 existing Python unit tests pass with local socket access.
- 48 executable Python/Rust protocol comparisons pass, plus initial-response,
  resume, shared-history and no-Python-child assertions. Coverage includes fuzzy
  and Unicode search, pagination, Fish history, configuration validation/reload,
  native launch preparation/dispatch, demo providers and window selection.
- Two native tests cover special-workspace focus ordering and address/Lua escaping.
- Real QML/Rust checks pass in a private compositor: normal and empty search results,
  provider responses, screenshot rendering, warm reopening with the same PID and
  concurrent cold opens. A synthetic screenshot was visually inspected.
- A staged installation under `/tmp` passes `--version` and `check`.
- `git diff --check` passes.

The full workspace `bun run build` was attempted but exceeded filesystem quota
while building/copying unrelated applications; it is not a passing acceptance
result. The older `tests/native_smoke.py` synthetic-window activation flow failed
in this environment for **both** Python and Rust. Its launch/focus behavior remains
unverified by that UI test; the narrower QML checks and protocol tests above do not
replace it. No production launcher, archive worker or user configuration was replaced.

## Reproduction

From the repository root after `bun install`:

```sh
bun run build --filter=@tjikos/runtjik
bun run check --filter=@tjikos/runtjik
cargo test --manifest-path apps/runtjik/Cargo.toml --release --locked
(cd apps/runtjik && bun run test)
python3 -B apps/runtjik/tests/rust_ui.py
python3 -B apps/runtjik/scripts/benchmark_rust.py /tmp/runtjik-comparison.json --ui --trials 15
```

The benchmark requires Quickshell, Hyprland and access to the current Wayland
session to create its private nested compositor. Use a new output filename. Fixture
state/logs remain in the printed `/tmp/runtjik-rust-bench-*` directory; private
compositor state is printed separately. No personal configuration/index/history is
written, no captures are taken by the benchmark, and no desktop applications are
activated. The visual check writes only a synthetic `--demo` screenshot under `/tmp`.

The checked-in defaults/presentation/Unicode contract is regenerated with
`python3 -B apps/runtjik/scripts/export_contract.py`; the static check detects drift.
