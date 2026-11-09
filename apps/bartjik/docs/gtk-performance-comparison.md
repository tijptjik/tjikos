# Quickshell and GTK comparison protocol

Measure the bar with all panels closed. A GTK preview with no surfaces outside workspace
7 is not comparable to the normal Quickshell bar: its memory excludes the unloaded bar
widgets. Both renderers must have the same output count, theme, modules, geometry and
provider workload before publishing a relative result.

## Startup

Use private configuration and D-Bus fixtures for repeated launches, retaining the normal
bar. Record executable/resource hashes and renderer versions. Separate process launch to
IPC/backend readiness from launch to first rendered frame; do not label socket creation
as visible readiness. Alternate renderer order, include the first process-cold sample
separately, and report warm-run median and range. Do not flush system caches. Keep tests
on workspace 7, with placement established before launch and no reservation of the real
desktop edge. Synthetic actions must not reach the real session services.

## Idle

After equivalent bars settle with panels closed, measure the frontend and every owned
backend/helper together. `scripts/measure_process_tree.py` records PSS, RSS, CPU and
context switches over a specified interval. PSS apportions shared pages; summing RSS
double-counts shared libraries. Context switches are not hardware wakeups. The sampler
rejects changed process trees. Record independent persistent desktop services separately
rather than attributing them to only one renderer.

Example with independently verified process identities:

```sh
python3 apps/bartjik/scripts/measure_process_tree.py \
  --root quickshell=QUICKSHELL_PID --root gtk=GTK_PID \
  --seconds 30 --output /tmp/bartjik-idle-comparison.json
```

Repeat visible-idle and auto-hidden-idle separately. Afterwards measure a visible
workspace preview and music/lyrics panel as distinct active workloads. Report GPU and
software renderer results separately. Existing synthetic controller timing is not a
replacement for frontend startup or total process memory.

## Live comparison, 2026-11-10

The user explicitly authorized brief serial bar restarts on the current displays. Three
configured bars on DP-1/DP-2, scale 1, used the same installed configuration, modules
and live services. No panels were open. A private configuration copy forced visible
versus auto-hidden modes; final concealment was verified for all three bars. Each mode
settled for four seconds and was sampled for 20 seconds. No concurrent build ran during
measurement. This is a short live-session sample, not a controlled statistical or GPU
benchmark.

| Measurement                       | Quickshell |  GTK |
| --------------------------------- | ---------: | ---: |
| Readiness median, 3 launches (ms) |        806 |  120 |
| Visible idle PSS (MiB)            |      270.1 | 80.8 |
| Visible idle CPU (% of one core)  |       12.4 |  8.4 |
| Hidden idle PSS (MiB)             |      272.6 | 84.4 |
| Hidden idle CPU (% of one core)   |       17.0 |  9.6 |

Startup is service restart request through backend-ready plus allocated bar geometry,
including polling and service overhead. It is **not** first-presented-frame timing.
Filesystem caches stayed warm; no global cache flush was performed. Renderer order was
Quickshell then GTK. Quickshell uses its CLI IPC and GTK a direct socket, so IPC
overhead also differs. Memory is the median summed PSS of owned processes; CPU covers
the entire service cgroup, including exited helpers. Shared external services such as
Spotify, Notjik and the compositor are excluded equally. CPU samples are
workload-sensitive.

The first diagnostic run caught excessive native media/tray refreshes on unrelated D-Bus
owner changes. Filtering those signals reduced GTK CPU from about 44% to about 9%; the
private-bus regression fixture verifies unrelated connection churn does not reread media
properties. That initial run also failed visible-mode equivalence and is not used for
the comparison above.

Reproduce (requires explicit live-display authorization):

```sh
dev-check python3 apps/bartjik/scripts/benchmark_renderers.py \
  --allow-live-displays --samples 3 --seconds 20 \
  --output /tmp/bartjik-renderer-benchmark-fixed.json
```

Measured executable SHA-256:

- `bartjik-gtk`: `a39303af3a393953b437101189c8a8550ecf24be0a48057eeae0aa04384d9ef5`
- `bartjik-gtk-core`: `e763d8054424219898330a48e7c1a7a329c9acad3e98bf948cdf4c0e5aff11df`
- `bartjik-core`: `e763d8054424219898330a48e7c1a7a329c9acad3e98bf948cdf4c0e5aff11df`

Raw process samples: `/tmp/bartjik-renderer-benchmark-fixed.json` (ephemeral). Active
music/lyrics and live-thumbnail costs, software-renderer comparisons and
first-presented-frame latency remain unmeasured; do not infer them from idle results.
