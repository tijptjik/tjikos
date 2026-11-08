# GTK migration and shared packages

GTK is now the default frontend for `runtjik` and the managed Super+Escape
shortcut. Use `runtjik --quickshell` for the legacy frontend, or `runtjik compare`
for isolated side-by-side review. Visual parity is still being reviewed.

## Side-by-side review

Run `runtjik compare` to open dedicated GTK (left) and Quickshell (right) windows
silently on workspace 7. Switch there manually when convenient. Both use the same
synthetic fixtures and a private copy of your theme/configuration, with history
and automatic activation disabled. Personal launchers and data are unchanged. `runtjik compare status` inspects these sessions;
`runtjik compare close` closes only the comparison. The command requires Hyprland
and at least 768×364 logical pixels on the destination display. On smaller
screens it reduces both windows equally. This is a visual/interaction review mode,
not a benchmark of production layer surfaces.

## Shared architecture

- `packages/tjik-gtk`: existing theme tokens and typography, virtualized result
  factories, layer-shell keyboard ownership, monitor placement, drag and resize,
  reduced-motion-aware startup/feedback/closing effects and snapshot removal effects.
- `packages/tasktjik-gtk`: task, reading, project and template presentation,
  revision-guarded mutations, editor, workflow templates, groups, snooze,
  verification and image UI. Other GTK apps can embed the same view.
- `runtjik-core` and `tasktjik-api`: existing discovery, launch/capture operations,
  persistence, recurrence, workflow eligibility and authority. The GTK frontend
  submits requests rather than reimplementing these rules.

Core I/O runs on a worker. Superseded searches are discarded; mutations are
serialized. Unchanged result/task snapshots preserve widgets; polling does not
rebuild an active editor or drag. Editors retain the revision they opened with;
only an acknowledged attachment made by that editor can advance it.

GTK3 icon resolution remains in the core process, isolated from GTK4. The native
layer-shell library is pinned and packaged beside the executable, with a relative
RPATH and its upstream license. No globally installed custom library is needed.

## Validation

`dev-check bun run build --filter=@tjikos/runtjik` builds both runtjik binaries,
its shared GTK crates and the affected Tasktjik backend.

`dev-check python3 -B apps/runtjik/tests/gtk_ui.py` uses synthetic fixtures and a
private compositor. The nested test window is placed silently on workspace 7
before launch. It covers concurrent startup, atomic reinstall, query cancellation,
provider views, task creation with actual GTK key input, completion, stale revision
rejection, keyboard navigation, layer dragging/resizing, outside-click dismissal,
pinning, narrow layout, warm reopen and comparison screenshots. It also checks
consecutive whole-row wheel steps with pointer movement between them, fixed
clipboard icon/text allocation, and real pointer toggling of dictionary sources.
Dictionary headers use the existing provider SVGs and counts; grouped senses,
examples, linked terms and the empty state follow `WordPane.qml`. The scrollbar
track has no theme outline, and only the empty search placeholder is centered.

The current Runtjik suite passes 32 tests (31 core and one GTK coordinate test).
The earlier Tasktjik suite passed 40 tests. The latter include
read-only image retrieval without mutation. Reading handoff, statistics lifecycle
and seven capture-history regressions pass against isolated synthetic data.

`dev-check python3 -B apps/runtjik/tests/gtk_desktop.py` additionally exercises
Wayland input-method-v2 through the compositor's text-input bridge: preedit,
candidate navigation, Enter commit, cancellation and committed Unicode. Its input
method is isolated from the host Fcitx daemon. It verifies monitor configuration
reloads, 1.5×/2× logical size and rendered screenshots, plus comparison startup,
workspace placement, shutdown and separation from the normal GTK session.
`gtk_ui.py` also checks repeated small pointer movements and exact grab-offset
preservation across a 1× to 1.5× monitor transfer. Rotated-output dimension math
is covered by the GTK binary's Rust test.

Startup now pulses the existing Runtjik artwork quietly until the initial backend
response, without a minimum display delay. It stops when hidden or ready, and
shows a static mark when desktop animations are disabled. Errors retain readable
text. Application removal shrinks/fades over 180 ms; confirmed gallery removal
uses the 380 ms fragment dissolve. Closing rows pulse only while the backend
reports them pending. Copy feedback fades in, and countdown numerals use the
outlined gradient treatment. Effects use logical render nodes and finish without
permanent animation timers. Background dragging leaves controls and result tiles
in charge of their own gestures; monitor transfer retains the actual grab offset.

## Visual review boundary

The main launcher now follows the rounded 720×600 shell, pill search, configured
font/icon theme, five-column grid, running markers and accent selection treatment.
A 500×480, three-column configuration is also exercised. Further Tasktjik UI
integration is deferred to that application's migration. Physical Fcitx candidate
popup placement, real mixed-DPI/rotated hardware, cross-monitor ghost previews,
and displaced-row movement still need parity review. Virtual protocol and output
checks do not establish physical-device parity. Keep Quickshell as the default
until the visual differences are accepted.

## Performance method

`scripts/benchmark_frontends.py OUTPUT.json` compares both frontends against the
same Rust backend and 3,000 synthetic entries. It alternates frontend order and
runs software/OpenGL separately, retaining OS caches after one unmeasured launch.
Results include the exact executable hashes. It checks ordered IDs and totals for
every query. Memory is summed PSS for frontend plus descendants, excluding the
compositor. Search timing ends at model update, not physical frame presentation;
GPU memory is not measured. This measures the launcher workload, not task-board
or capture throughput. See the accompanying benchmark JSON for measured results.

### Earlier baseline medians, 2026-11-09

| Renderer | Frontend | Cold ms | Warm ms | Search ms | Open PSS MiB | Open CPU, one core |
|---|---|---:|---:|---:|---:|---:|
| software | quickshell | 385.0 | 108.2 | 156.9 | 90.9 | 0.0 |
| software | gtk | 109.2 | 39.9 | 158.8 | 49.3 | 1.0 |
| opengl | quickshell | 426.5 | 113.6 | 161.6 | 116.4 | 0.5 |
| opengl | gtk | 144.6 | 40.8 | 156.4 | 74.6 | 2.0 |

These measurements predate the gesture/animation changes above; the JSON records
the measured binary hashes. Three paired trials per renderer; 15 warm openings and 15 searches per frontend. Both frontends measured zero hidden CPU during these samples. OpenGL GTK open CPU ranged from 1–2% of one core; this remains an optimization opportunity.

Synthetic screenshots: [GTK](gtk-launcher.png), [Quickshell reference](gtk-quickshell-reference.png), and [narrow GTK](gtk-narrow.png). Selection differs because the GTK capture follows a keyboard navigation test.

Current review artifacts: [startup mark](gtk-startup.png) and [side-by-side comparison](gtk-comparison.png) (GTK left, Quickshell right).
