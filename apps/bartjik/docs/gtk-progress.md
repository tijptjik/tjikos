# Bartjik GTK progress

## Baseline and scope

Read-only inventory on 2026-11-09: the active `bartjik.service` runs
`/usr/bin/fish -lc exec /home/io/.local/bin/bartjik`; PID 225097 at inventory. The
existing Rust core launches Quickshell. Two bars are configured. Neither the active bar
nor its service/default is switched by this opt-in migration.

The initial GTK slice reuses `tjik-desktop` and `tjik-gtk`. It concentrates on
output/window state, a small bar surface, reservation and panel input ownership. It does
not run the existing provider engine a second time or claim notification or tray
singleton names. Production defaults remain Quickshell.

## Initial parity matrix

| Configured provider                  | Existing owner                                     | GTK replacement state                                                                                 |
| ------------------------------------ | -------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| workspaces                           | Rust Hyprland helpers + Quickshell preview objects | Shared adapter discovery and window activation; empty-workspace selection and live thumbnails pending |
| datetime                             | Rust provider                                      | Clock presentation in initial surface                                                                 |
| locale                               | Rust helper                                        | Pending                                                                                               |
| audio                                | Rust helpers + Quickshell PipeWire                 | Pending native subscription and controls                                                              |
| playerctl                            | Quickshell MPRIS + Rust media helpers              | Pending                                                                                               |
| icontray                             | Quickshell SystemTray/menu host + Rust attention   | Pending watcher/menu ownership audit and native host                                                  |
| DND                                  | Notjik authority + shared QML history              | Pending GTK client; Notjik remains notification owner                                                 |
| network, mullvad, tailscale          | Existing Rust providers                            | Pending GTK cards and mutations                                                                       |
| caffeine, nightlight, monitor-layout | Existing persistent Rust policy                    | Pending GTK controls; preserve independent policy services                                            |
| codex, openrouter, crypto            | Existing Rust providers                            | Pending GTK presentation; no account queries in fixtures                                              |

All existing module configuration remains authoritative for the legacy frontend. The
initial GTK surface is a deliberately limited opt-in slice, not full parity. Additional
gates include configured islands, vertical bars, auto-hide, tooltips,
pinning/detachment, panel transitions, provider error/reconnect handling, physical
mixed-DPI and IME checks, rendered comparisons and reproducible performance limits.

## Completed first slice and validation

- Added `bartjik-gtk`, opt-in through `bartjik --gtk`; default service still launches
  Quickshell.
- One adapter instance feeds all output surfaces with normalized windows and pushed
  snapshots. Window actions run on a bounded worker queue and use opaque IDs.
  New/disconnected outputs reconcile with GTK monitor discovery.
- Bar surfaces use layer-shell, no keyboard grab, optional 44-logical-pixel top
  reservation. Window panels are small independent surfaces, hide completely on
  dismissal, use on-demand keyboard focus and support Escape. Theme tokens/font come
  from existing configuration.
- Startup explicitly rejects missing output discovery or layer-shell. Action errors are
  shown. Clock formatting, bar geometry and provider selection are intentionally limited
  in this first slice; existing module layouts are not yet ported.
- The native library is bundled with a relative RPATH. `scripts/install.sh --gtk-only`
  updates only the opt-in package and launcher, preserving the legacy
  core/QML/configuration/service policy.

Commands run from `apps/bartjik`, through `dev-check`:

- `bash scripts/build.sh`: release build passed.
- `dbus-run-session -- python3 -B tests/gtk_surface.py`: reservation, full-width sizing,
  actual pointer-driven window activation, panel map/unmap, output hotplug and targeted
  quit passed. Artifact: `/tmp/bc-8cq_2nqu/gtk-bar-panel.png`.
- `cargo test --release --locked`: 20 tests passed.
- `python3 -B tests/native_contracts.py -v`: 9 tests passed.
- `dbus-run-session -- python3 -B scripts/gtk_compare.py --smoke`: both frontends loaded
  in a private compositor. Artifact: `/tmp/bc-w9pkhi4n/comparison.png`.
- `bash scripts/check.sh`: formatting, GTK-enabled release checking and
  configuration/theme/font provenance passed.

Logs: `/tmp/bartjik-gtk-final-validation.log` (backend and initial input checks),
`/tmp/bartjik-gtk-surface-final.log` (final sizing, input, comparison and static
checks). The comparison explicitly selects the Qt Basic style and independent process
groups; headless smoke capture is bounded, while interactive use keeps the nested output
visible on workspace 7. Screenshots retain private-compositor diagnostic overlays; they
are functional evidence, not approved visual parity or performance baselines.

Installation receipt and prior-tree backup: `/tmp/bartjik-gtk-install.json`. Next slice:
configured islands/geometry and true workspace selection through the shared contract,
then provider cards with their existing domain owners. Audio, tray/DBusMenu,
notification history and live previews remain unported. GTK process restart supervision,
auto-hide, detachment, vertical/mixed-DPI and IME review remain explicit gaps. No host
compositor or active bar restart is part of this slice.

## Configured islands and workspace selection (2026-11-09)

- Shared `tjik-desktop` snapshots now include empty workspaces independently of windows.
  Hyprland and fake adapters implement session-scoped workspace selection, including
  configured persistent normal workspaces. Stale generations and missing capabilities
  fail explicitly. Runtjik continues to use the same compatible desktop contract.
- GTK resolves the existing bar configuration: monitor routing, islands, four edges,
  columns, offsets, overflow and stacking. Reservation is a separate input-empty surface
  per output/edge, using the maximum required depth rather than adding duplicate
  reservations for adjacent islands. `--reserve` remains opt-in.
- Island gaps pass input to underlying windows. Region changes explicitly queue a
  surface commit; panel input waits for allocation. Focus-only events preserve widgets
  when displayed content is unchanged. Configuration can be reloaded with `reload`.
- Unsupported providers and auto-hide are reported rather than presented as parity. The
  comparison uses separate top/bottom fixture configurations. The shared test launcher
  identifies newly created compositor windows without closing an existing demo.

Validation: 12 shared adapter tests; 20 backend and 2 GTK layout tests; Bartjik release
build and static checks; Runtjik release compatibility and desktop boundary check.
Private-bus surface fixture passed empty-workspace pointer selection, window activation,
panel map/unmap, all four reservations, reload, actual gap click-through to an
underlying fixture, output hotplug and targeted quit. Its parent was verified on
workspace 7 without focus theft. Log: `/tmp/bartjik-gtk-islands-test.log`; inspected
render: `/tmp/bc-daanuak2/gtk-bar-panel.png`. Compositor diagnostic overlays remain in
the image; this is functional evidence, not approved visual parity.

Next: provider cards using existing core owners, then hover/auto-hide and panel
interaction parity. Physical mixed-DPI/rotation, IME, detachment, performance baselines
and process supervision remain unverified. The current Quickshell service stays default.

Final commands (heavy commands wrapped in `dev-check`):

- `cargo test --manifest-path packages/tjik-desktop/Cargo.toml`: 12 passed.
- `bun run build --filter=@tjikos/bartjik`: passed.
- From `apps/bartjik`, with the bundled native pkg-config path:
  `cargo test --release --locked --features gtk-ui`, `bash scripts/check.sh`: passed.
- `dbus-run-session -- python3 -B apps/bartjik/tests/gtk_surface.py`: passed.
- From `apps/bartjik`, `dbus-run-session -- python3 -B scripts/gtk_compare.py --smoke`:
  passed; `/tmp/bc-4ma3_r09/comparison.png`, `/tmp/bartjik-gtk-islands-compare.log`.
- `cargo check --release --locked --manifest-path apps/runtjik/Cargo.toml` and
  `python3 -B apps/runtjik/tests/desktop_boundary.py`: passed.

Installation uses `scripts/install.sh --gtk-only`; byte verification and prior-file
backup location are recorded in `/tmp/bartjik-gtk-islands-install.json`. To roll back,
restore its backup's `bin`, `lib`, `docs` and `README.md` into `~/.local/lib/bartjik`
and its `launcher` into `~/.local/bin/bartjik` while the opt-in GTK process is closed.
No legacy service restart is required. Reopen an existing opt-in GTK demo to load new
bytes; an already running comparison retains its previous executable.

## Auto-hide controller (2026-11-09)

Implemented the next surface slice: GTK now follows the legacy edge hysteresis contract
for `activationEdge`, continuous `activationDelay`, `hideDelay`, `keepVisibleDistance`,
`revealSize`, `fadeDuration` and `syncMonitors`. Shared groups use the triggering
output's scale and configuration, including differently named bars. Independent bars
retain their own state. Reload and hotplug reconcile groups.

One worker samples the selected adapter's optional pointer capability, outside GTK, with
a single latest-value slot. It sleeps when no visible bar needs auto-hide, backs off on
failure and stops on teardown. Unsupported, failed or stale pointer data suspends
concealment and is exposed by `autoHideError`. No global edge window intercepts the
desktop. Fully concealed islands have empty input regions and release their reservation;
revealing restores both. Open panels hold visibility.

Provider-driven recording visibility, provider cards, pinning/detachment, keyboard
reveal parity and physical mixed-DPI verification remain pending. This does not switch
the default bar or start another provider engine.

Validation for this slice:

- `dev-check bun run build --filter=@tjikos/bartjik`: release build passed.
- `dev-check dbus-run-session -- python3 -B apps/bartjik/tests/gtk_surface.py`: passed
  with real synthetic pointer events: dwell cancellation, fade completion, concealment
  input release to an underlying undecorated fixture, reservation release/restoration,
  panel retention, shared and independent monitors, hotplug, reload disabling the
  sampler, and the fake adapter's missing-pointer fallback. Existing workspace
  selection, panel activation and four-edge checks also passed. Log:
  `/tmp/bartjik-gtk-autohide-test.log`; inspected image:
  `/tmp/bc-y2dmbphl/gtk-autohide.png`.
- GTK unit coverage includes four edges, scaled retention, continuous dwell,
  cancellation, fade reversal, group opt-out/hotplug, expired samples and worker
  shutdown. Synthetic checks do not establish physical mixed-DPI parity.

Installation receipt and backup: `/tmp/bartjik-gtk-autohide-install.json`. The opt-in
installer preserves legacy core/QML bytes and verifies the existing service
PID/invocation. Reopen an already running GTK instance to use this build.

Final validation: from `apps/bartjik`, through `dev-check` with the bundled native
pkg-config path, `cargo test --release --locked --features gtk-ui` passed all 27 unit
tests (20 backend, 7 GTK); `bash scripts/check.sh` passed. Scoped Biome formatting
checks and `git diff --check` also passed.

## Datetime card — in progress

The shared datetime formatter and world-clock model are extracted into
`src/datetime.rs`; both the existing provider and GTK use it. GTK has a bounded, shared
provider worker, alternate-format buttons, world-clock cards, hover delay, loading/error
presentation and configuration-generation cancellation. Added tests cover timezone/DST,
deduplication, reload recovery and real card input.

Validation is pending: Rust formatting and Python syntax pass, but the release build is
queued behind an interactive `scripts/gtk_compare.py` session holding `dev-check`. The
user has been asked whether that comparison may be closed. Do not bypass the lock or
treat this slice as installed/verified. The installed app remains the previously
validated auto-hide build. Resume with the queued build, unit/static checks and
`tests/gtk_surface.py`, inspect `gtk-world-clock.png`, then install and verify bytes.
The core datetime extraction also requires the existing provider contract tests before
installation decisions.

## Same-session comparison and datetime validation

The user explicitly changed the visible-test procedure: extra nested Hyprland sessions
were closed, and Bartjik comparisons now use the current compositor.
`apps/bartjik/AGENTS.md` records that scoped override. The interactive viewer no longer
holds the `dev-check` lock.

`python3 apps/bartjik/scripts/gtk_compare.py start` leaves the production Quickshell bar
in place and renders GTK above each configured surface with an 8-pixel gutter. It
temporarily holds the reference visible through its existing IPC. No saved
configuration, service default, workspace or focus is changed. `stop` closes only the
comparison and restores the prior reveal state; `status` reports its identity.

The comparison bridge consumes cached item snapshots from the running bar. It strips
actions, draws GTK widgets using the existing islands, theme and artwork, and rejects
compositor mutations. It creates no second native provider set and reserves no
additional edge. This is read-only renderer evidence, not implemented
tray/audio/media/account provider parity. Text composition, icon styling and workspace
treatment still differ visibly and remain migration work.

Validation:

- `dev-check bun run build --filter=@tjikos/bartjik`: passed.
- GTK-enabled release unit tests: 22 backend and 10 GTK tests passed, including shared
  datetime formatting, DST, provider deduplication and failure recovery.
- `dev-check bash scripts/check.sh` from the app: passed.
- `dev-check python3 -B tests/native_contracts.py -v` from the app: 9 passed.
- `dev-check python3 -B apps/bartjik/tests/gtk_host.py`: all three configured surfaces
  passed actual same-session geometry checks with exactly 8px gutters. Cropped images
  were inspected under `/run/user/1000/bartjik-gtk-comparison/DP-1-status-bar.png`,
  `DP-1-workspace-bar.png` and `DP-2-workspace-bar.png`.
- Only host Hyprland PID 1524980 remained. Legacy Bartjik PID 225097 and its invocation
  were unchanged. GTK installed bytes matched the validated build; receipt:
  `/tmp/bartjik-gtk-host-compare-install.json`, original rollback copy:
  `/tmp/bartjik-before-host-compare-1af26089`.

The earlier datetime build blocker is resolved. Its shared model and worker are
unit-validated and included in the installed opt-in GTK artifact; the normal interactive
world-clock card still needs same-session input review. Historical `gtk_surface.py`
remains available but was not launched after the user's instruction. Default `test:gtk`
now checks the requested host comparison. Next: align remaining renderer differences and
validate native cards/actions incrementally; pinning, detachment and full provider
parity remain incomplete.

### Workspace motion and media text (2026-11-09)

The same-session comparison now keeps module widgets across snapshots. Workspace slots
use the reference 32px minimum width, 24px height, configured gap, 22px accent ring,
240ms OutCubic ring motion, and 200ms InOutCubic label/dot transition. Labels use Pango
ink bounds for centering. Media has separate artist, middle-dot separator and track
slots with the configured 5px gaps and independent width limits. Text uses the
configured font and style precedence. Overflowing marquee text pauses 1200ms, travels at
35px/s (minimum 800ms), pauses 1400ms, then returns over 300ms. Unchanged text widgets
survive unrelated snapshot and playback-icon updates.

A live check found and fixed startup-width clipping after the first item snapshot;
`gtk_host.py` now checks that every island fits inside its allocated bar as well as 44px
island height and the 8px comparison gutter. `gtk_motion.py` temporarily pauses only the
comparison mirror and supplies synthetic presentation data, checking ring interpolation
and ticker progress across updates. Its finally block restores the snapshot and resumes
the mirror. It never selects a host workspace or changes media.

Validation commands for this slice:

- `dev-check bun run build --filter=@tjikos/bartjik`
- `dev-check bash scripts/check.sh` from the app
- GTK-enabled release unit tests (22 backend, 12 GTK) and 9 native contract tests
- `dev-check python3 -B apps/bartjik/tests/gtk_motion.py`
- `dev-check python3 -B apps/bartjik/tests/gtk_host.py`

Install through `scripts/install.sh --gtk-only`, verify dist/installed hashes, and
restart only `gtk_compare.py`. The default Quickshell service remains unchanged. Install
receipt and rollback path: `/tmp/bartjik-gtk-host-final-install.json`. Screenshots
remain in `/run/user/1000/bartjik-gtk-comparison/`.

Scope: these workspace/media changes are in the read-only comparison renderer. Native
workspace buttons still need the same rendering plus accessible activation,
hover/preview and input validation. Native media providers/actions, icon parity, other
transitions, detachment and full provider parity remain subsequent work.

### Connectivity, tray attention/activity and icon morphs (2026-11-09)

The GTK comparison now follows Quickshell's same-island connectivity composition: a
Tailscale module paired with Mullvad is hidden, and `connection == Running` adds three
muted dots beneath Mullvad's artwork. An unpaired Tailscale module remains separate. The
mirror retains only the connection string from provider panel state.

Chat artwork displays the existing `state.urgent` accent badge. The authoritative legacy
tray sets that from SNI NeedsAttention or the existing attention helper; GTK does not
infer unread counts or touch chats. The mirror also carries the native screenRecording
and hyprvoice states. The tray adds a 10px red recording dot for screen/voice recording,
and a 1100ms opacity pulse for voice processing, unless screen recording takes
precedence or animation is disabled. Inactive/unavailable states remove the slot. The
live comparison now displays the host's existing recording state; no recording was
started or stopped for testing.

Persistent GTK icon objects interpolate the existing licensed MorphData paths with 220ms
InOutCubic timing. Supported pairs match Quickshell: notification bell, idle coffee/bed,
nightlight/sun, desktop/game and speaker/headphones. Interrupted morphs continue from
their current interpolated shape. Noncompatible changes use the normal artwork;
animateIcons=false and material icons skip morphing.

Validation: release build, app static checks and 13 GTK unit tests passed.
`dev-check python3 -B apps/bartjik/tests/gtk_indicators.py` passed synthetic checks for
intermediate morph progress, merged connectivity visibility, attention on/off,
processing pulse, steady screen-recording precedence and inactive cleanup. This fixture
pauses only the mirror supervisor and restores it in finally; it changes no host
service/device/chat state. `gtk_host.py` passed geometry and clipping checks on all
three live surfaces. Rendered evidence: `synthetic-indicators.png` and
`DP-1-status-bar.png` in the comparison runtime directory.

Installed via the supported --gtk-only installer with validated-byte comparison; legacy
core/QML files and service invocation remain unchanged. These are renderer and
mirrored-state improvements; native SNI hosting, notification authority and connectivity
mutations are not newly implemented by this slice.

Connectivity spacing follow-up: Tailscale dots now sit below the icon slot, matching
Quickshell's negative bottom margin. The overlay gets extra drawing space without
changing the flag center, module spacing or island height. Previously GTK positioned the
dots inside the slot, crowding the flag.

### Ordered connectivity dots and tray insertion/removal (2026-11-09)

Tailscale dots fade in left-to-right and out right-to-left, with 90ms stagger and 140ms
per-dot easing (320ms total). Reversal starts from current dot opacity; unchanged
snapshots do not restart it. Only dots are drawn, with no numeric labels.

The comparison tray now retains stable icon widgets through 240ms eased insertion and
removal. Slot width, gap and opacity animate together, including the recording slot.
Removed slots remain until collapse completes, then are unparented. Returning IDs
reverse from their current width. A frame callback recalculates the bar's geometry while
resizing and stops afterward. Animation-disabled configuration settles immediately.
Existing icon morphs and recording pulses remain independent.

Validation: release build, static checks and 14 GTK unit tests; synthetic
`gtk_tray_motion.py` observed ordered on/off dot states and intermediate tray widths
from 20px to 84px and back, plus corresponding whole-bar resize frames. Removed widgets
were released and idle ticks stopped. Frame evidence is retained in
`/run/user/1000/bartjik-gtk-comparison/tray-motion-evidence.json`. `gtk_indicators.py`
also checks rendered badge/recording pixels; `gtk_host.py` checks 44px island height,
8px gutter and unclipped allocations. Existing private fixture isolation and --gtk-only
install/byte verification workflow are unchanged.

Live preview freshness follow-up: the previous bridge sampled Quickshell every second
and GTK independently polled the file every second, allowing nearly two seconds of
latency and missed short activity states. GTK now monitors the mirror's parent directory
on a private GLib worker context, surviving atomic file replacement and keeping
reads/parsing off the UI thread. The mirror samples about every 200ms, skips unchanged
writes, and retains the original once-per-second reveal heartbeat. This remains polling
at the Quickshell boundary; it is not a new native provider.

Build/static validation and indicator/tray-motion/host checks passed. A short synthetic
recording pulse reached GTK 3.6ms after the file update; this excludes Quickshell
sampling latency. Actual live unread badges were also visually checked. The tray
contract supplies urgent/unread presence, not a numeric unread count.

## Native bar input and services (2026-11-09)

This supersedes the earlier read-only-bridge limitation for `serve` and comparison
`start --native`. Mirror mode remains available for renderer regressions.

| Bar feature          | Current GTK implementation                                                                                                 | Evidence boundary                                                                               |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| Configured providers | Existing core scheduling and actions; native MPRIS/SNI rows                                                                | Provider policy retained, no rewrite of account or device policy                                |
| Time/date            | Existing formatter and per-bar alternate state                                                                             | Synthetic GTK roundtrip and formatter tests                                                     |
| Mouse intents        | Shared precedence, part/item overrides, null disabling, double click, hover dwell, scroll accumulation                     | Rust precedence and synthetic GTK dispatch tests; physical input acceptance remains             |
| Media                | Native subscriptions and bounded worker actions; previous/next, restart, play/pause, configured actions                    | Private-bus fixture; no host playback changed by tests                                          |
| VPN                  | One visual; flag and lower dots resolve to distinct modules                                                                | Renderer and action resolver; real network toggles not automated                                |
| Tray/unread          | SNI host, queued watcher ownership, icon overrides/pixmaps, attention dots, actions                                        | Private-bus registration/path registration/owner-loss/stale-target tests                        |
| Tray menu            | DBusMenu layout, AboutToShow, revision updates, nested back, checks/radio, disabled/removed validation; non-grabbing popup | Protocol fixture; physical focus/placement acceptance still required                            |
| Recording            | Direct read-only helpers; no Quickshell mirror dependency; insertion remains animated                                      | Existing helper tests and rendered synthetic indicator checks; live latency not yet benchmarked |
| Recovery             | Owned core restart, screen replay, bounded input queue and writes                                                          | Synthetic GTK abrupt-core-crash recovery                                                        |
| Surfaces/motion      | Existing 44px islands, ring/ticker/morph/dot/tray motion, auto-hide and reservations                                       | Prior and repeated renderer checks                                                              |

Panels remain outside this bar-only milestone, including audio/device panels,
notification history, live window previews and their drag/detachment transitions. The
tray exposes unread presence, not numeric counts. Full custom tooltip styling, physical
mixed-DPI/IME and menu keyboard/focus review remain acceptance gaps; this is not a
declaration of complete desktop-shell parity.

The separate installed `bartjik-gtk-core` keeps Quickshell's current core untouched.
Core commands remain authoritative and re-resolve configured actions; media owners are
revalidated. A bounded writer keeps pipes off GTK, restarts replay screens, and owned
process groups are torn down. Native state uses coalesced signal-triggered refreshes
rather than an idle polling timer. Activity helpers retain their explicit 150/250ms
cadence. No recording, chat action, network toggle or host media action is used as an
automated test.

Validation (all finite heavy checks through `dev-check`):

- `bun run build --filter=@tjikos/bartjik` and `bash scripts/check.sh`: passed.
- `cargo test --release --locked --features gtk-ui`: 22 core + 15 GTK tests passed.
- `/usr/bin/python3 -B tests/native_services.py`: private-bus service/lifecycle tests
  passed.
- `/usr/bin/python3 -B tests/gtk_native.py`: date roundtrip, media/configured/null
  intents and abrupt owned-core restart passed. Synthetic voice state reached GTK in
  162.7ms including the helper cadence; this is state delivery, not a physical
  microphone-to-pixel benchmark. The fixture explicitly selects the backend `serve`
  process rather than an activity helper when simulating a crash.
- `gtk_indicators.py`, `gtk_tray_motion.py`, `gtk_motion.py`, `gtk_host.py`: passed
  against installed mirror mode. Rendered badges, ordered dots, 20→84→20px tray motion,
  ring interpolation and ticker persistence were exercised.
- `gtk_host.py` also passed in native mode: three surfaces, 44px islands, 8px gutter, no
  clipping or extra reservation. Native status and media screenshots were inspected.

Live artifacts: `/run/user/1000/bartjik-gtk-comparison/DP-1-status-bar.png`,
`DP-1-workspace-bar.png`, `DP-2-workspace-bar.png`, `synthetic-indicators.png` and
`tray-motion-evidence.json` in the same directory. Native mode remains open for
interactive review. No nested compositor was launched or host mutation tested.

Installed through `scripts/install.sh --gtk-only`; hashes and rollback backup are in
`/tmp/bartjik-gtk-host-final-install.json`. GTK SHA-256:
`17b510cda5f04e31ce84d8211facf31e80f85ccb35c0bb303c59d84562a31f16`; its separate core:
`da531c2a394a940ca8c406ccc28179e92327cea36068662ee2180b6189f66dc9`. Legacy core/QML
hashes, service PID 225097 and invocation `4ececfe112124c6491928307a59f1cf2` were
unchanged. Rollback: close only the comparison, restore the receipt's backup files and
launcher, then reopen the desired opt-in mode. Quickshell requires no restart.

Commit progression: `b4dbc9f` adapter workspace discovery/selection; `5403ba4` shared
datetime; `a8d3d56` GTK surfaces/rendering; `eaa937f` safe comparison/motion fixtures;
`5b21002` native services; `1cfb081` independent GTK runtime and bar intents. Subsequent
comparison/documentation commit records the current evidence and explicit gaps.
Unrelated concurrent changes are excluded. No push or default switch is performed.

Earlier renderer-only performance numbers are not comparable to this independent
provider runtime. Next: physical gesture/menu-focus review and remaining tooltip polish,
then the separate provider-panel migration; do not claim those as verified.

## Shared panel framework and hover tracking (2026-11-09)

The attached-panel foundation now lives in two focused modules:

- `src/gtk/panels.rs`: toolkit-independent target selection, edge-aware placement,
  configured dwell, gap grace, source/card/corridor retention, pinning and dismissal.
- `src/gtk/panel_view.rs`: stable layer surface, header, grouped status rows, scrollable
  body, optional footer and a `Content` boundary. Providers supply widgets, preferred
  width and keyboard requirements; the surface owns geometry, transitions and input.

One attached card is shared across providers. Existing backend `panel.rows` render as
read-only status content (including world clocks). This is the framework slice, not a
claim that specialized audio/network/notification controls or live thumbnails are
ported. Those bodies can now be implemented without duplicating surface behavior.
Detachment/dragging is still a later slice.

Hover behavior matches the reference rules: nearest-item partitioning covers island
padding and internal gaps; disabled targets do not acquire a panel. Initial dwell uses
`panel.delay` / `preview.delay`; moving within an open bar switches content without a
new dwell. Pointer-following placement and body changes use 140ms transitions.
Inter-island travel gets `max(250ms, closeDelay)` grace. The source island, current card
and narrow travel corridor retain the panel. Other desktop space remains outside the
surface's input region. Output offsets and all four edges are explicit logical
coordinates. Missing/stale global pointer data never becomes a fabricated exit.

Bar-local hover events wake the existing shared adapter pointer worker. No second
sampler or compositor commands were added to widgets. Sampling stops when neither hover
panels nor auto-hide need it. Open/pinned panels retain their source bar; explicit close
suppresses immediate reopening under a stationary pointer. Removal of the source target
or compositor invalidation destroys the attached surface. Pinned or editable content
requests on-demand keyboard ownership; ordinary hover previews do not take keyboard
focus. `bartjik --gtk hide-panel` also dismisses the shared card.

Validation: release build and static checks; 22 core + 19 GTK tests, including new
pure-controller cases for four-edge/clamped placement, offset displays, dwell/cancel,
nearest-target partitioning, gap/corridor retention, pinning and stale coordinates.
`tests/gtk_panels.py` runs synthetic providers and injected pointer samples in the
prepared current-session comparison layout. It verifies real GTK card allocation,
initial/cancelled dwell, pointer-following placement, corridor retention, immediate
handoff, disabled hover, sustained exit, close suppression and idle sampler teardown.
`tests/gtk_native.py` still passes bar input, recording-state delivery and backend crash
recovery. Test commands run through `dev-check`; no host pointer, focus, recording or
device action is changed. Physical pointer travel remains a separate acceptance check.

Rendered synthetic world-clock card:
`/run/user/1000/bartjik-gtk-comparison/panel-framework.png`. The private test config
shortens dwell only in its temporary copy; managed/live delays are unchanged. Install
with the supported `--gtk-only` path, verify GTK/core hashes, and reopen only the GTK
comparison. Keep Quickshell as default. Next: provider-specific bodies and editable
control/focus tests on this framework, followed by drag/detach behavior.

The panel-framework install receipt and pre-update rollback backup are recorded in
`/tmp/bartjik-gtk-panels-install.json`. The opt-in GTK preview is reopened after
install; legacy core/QML bytes and the production service identity are checked
unchanged. No managed configuration, user data, or unrelated concurrent repository work
is replaced.

## Workspace-7 live preview and crash recovery

The comparison supervisor now defaults to workspace 7. `start --native --workspace 7`
uses real providers and configured bar actions, with auto-hide disabled in its private
configuration. Adapter output/workspace events create and remove surfaces on the
matching output; unknown workspace state fails closed. Menus and attached panels are
removed with their source bar. No workspace or focus dispatch is performed. Legacy
Quickshell remains unchanged because its visibility IPC is bar-wide across monitors.

The disappeared preview had a GTK crossing-event SIGSEGV. Hover appearance updates are
now deferred until after event dispatch, and replaceable icon artwork cannot become an
input target. This addresses the suspected mutation-during-crossing cause; sustained
physical hover remains a live acceptance check. The supervisor restarts abnormal exits
with bounded backoff. Owned core/status children receive a parent-death signal to avoid
orphan provider instances; recording workers and launched applications are not scoped to
the preview process group.

Validation includes the normalized workspace gate unit test and the read-only installed
host check, which accepts a ready backend with zero surfaces outside workspace 7.
Specialized panel controls, thumbnails, and detachment remain subsequent slices.

Verified for this slice: `dev-check bun run build --filter=@tjikos/bartjik`,
`dev-check bash scripts/check.sh`, and locked release Cargo tests with `gtk-ui` passed
(22 core + 20 GTK tests). `dev-check python3 apps/bartjik/tests/gtk_host.py` passed
against the installed native preview while outputs displayed workspaces 1/11. A
hidden-preview SIGKILL recovery test verified supervisor restart, removal of all three
owned helpers, no out-of-scope surfaces, unchanged workspace/focus, and the running
executable hash. Receipts: `/tmp/bartjik-gtk-workspace-validation.json` and
`/tmp/bartjik-gtk-workspace-install.json` (includes rollback directory). Interactive
rendering and physical pointer review on workspace 7 remain for the user; validation
never switched there. Next: specialized provider panel contents on the existing deck.

## Shared card fidelity: header, controls, and world clock

The attached GTK deck now uses the reference `PanelHeader`, `PanelButton`, and
`StatusDetails` metrics: 16px outer inset, 32px icon/title header, divider at y=58, body
at y=72, 12px card radius, and the configured font and panel-border fallback. Header
artwork follows the original provider mapping; pin/close retain their existing behavior
as accessible 32px icon controls with hover, pressed, checked and focus states. The
generic subtitle no longer expands the shared header beyond the reference geometry.
World clocks use 56px surface rows separated by 6px, 13px location labels, 10px date
labels and 23px time values. Ordinary provider updates no longer replay the crossfade
used when switching to a different card.

`dev-check python3 apps/bartjik/tests/gtk_panel_primitives.py` exercises the actual GTK
primitives with synthetic data in an ordinary window placed silently on workspace 7
before launch. It asserts measured border-box width/height, header height, row height,
and content origin, and exports `/tmp/bartjik-panel-primitives.png`. The paired
`PanelPrimitives.qml` fixture loads the original QML header/status components and
renders offscreen to `/tmp/bartjik-panel-reference.png`; no production providers are
started. This is the first card fidelity slice, not specialized audio/network/media or
notification-panel parity. Next: provider-specific card bodies using these primitives.

## Codex card and tooltip replacement

Codex now has a dedicated read-only GTK body instead of the generic backend field list:
weekly accounts first, raised plan labels, right-aligned remaining quota and reset
countdown, seven day segments, then the two-column 24-hour summary. The shared header
uses `Codex`, 440px width, and no persistent buttons while attached/unpinned. The
segment calculation follows `CodexTimeline.js`: rose overlap, blue reset-time surplus,
teal credit surplus, black known-empty slots, and surface-colored unknown slots.
Unknown/expired reset times, stale state, no accounts, and unavailable totals remain
explicit. Countdown refresh runs only while the card is mapped.

Bar tooltip eligibility now follows `Part.qml`: configured hover panels, workspace
previews, crypto, and implicit system-panel hover actions suppress provider tooltips.
Plain tray items retain their descriptive tooltips; accessible labels remain present.

`dev-check python3 apps/bartjik/tests/gtk_panel_primitives.py --codex` renders identical
seeded data through GTK and the original `CodexDetails.qml`/`CodexTimeline.js`. It
checks the 440×529 card geometry and exports `/tmp/bartjik-codex-primitives.png` and
`/tmp/bartjik-codex-reference.png`. GTK fixtures are pre-placed silently on workspace 7;
QML reference capture is offscreen. Timeline/formatting and tooltip-policy unit tests
cover the semantic edge cases. Install/rollback receipt:
`/tmp/bartjik-gtk-codex-install.json`. Other specialized panels remain separate work.

## Fixed-layout card transitions

The shared attached surface now separates destination layout from animated shell
geometry. `panel_reveal.rs` allocates the entire incoming card at its final width and
height; animation changes only the rounded clip, shell border/background, placement, and
input region. Text wrapping, quota tracks, headings, and scroll viewport dimensions stay
fixed during growth and shrink. The outgoing card is the last painted content snapshot,
including the header, rather than a live GtkStack page being resized. Same-size switches
still crossfade; rapid interrupted handoffs preserve the displayed composite.
Interrupted snapshot history is flattened at output scale, and the outgoing snapshot is
released at completion. Old provider widgets unmap immediately.

Validation: `dev-check python3 apps/bartjik/tests/gtk_panel_primitives.py --transition`
uses a synthetic ordinary window pre-placed silently on workspace 7. It samples growth
frames and asserts identical destination label bounds, tests stable wrapping during
shrink, rejects picking outside the visible clip, and covers interrupted/same-size
handoffs and snapshot release. Midpoint artifact:
`/tmp/bartjik-panel-transition-mid.png`. The test allocates sampled frames explicitly
because inactive workspaces need not receive compositor frames; it does not move the
host pointer or switch focus/workspaces. Existing placement/hover policy is unchanged.
Install/rollback receipt: `/tmp/bartjik-gtk-transition-install.json`.

### Pointer-following flicker correction

Card opacity now has a separate clock from shell geometry. Previously each pointer
movement retargeted placement and restarted the shared clock, briefly restoring the
outgoing card. Only a new card identity starts a content fade; position/size retargets
cannot rewind it. The reveal widget also enforces monotonic fade progress within a
handoff. Regression coverage repeatedly resets geometry during one content transition
and injects a late low-progress frame into the rendered fixture. Both must preserve
forward content progress and release the old snapshot after completion.

### OpenRouter card

The GTK panel now uses the original OpenRouter hierarchy: 32px credit balance with
baseline-aligned USD, key-limit/credit labels, cached-update notice, and daily token
usage with 26px rows and 4px gaps. Provider notes preserve restricted, unavailable, and
empty activity states. Only numeric token rows appear, including zero usage; the shared
panel supplies scrolling, hover retention, pinning and transitions.

Synthetic comparison:
`dev-check python3 apps/bartjik/tests/gtk_panel_primitives.py --openrouter`. Artifacts:
`/tmp/bartjik-openrouter-primitives.png` and `/tmp/bartjik-openrouter-reference.png`.
Fixtures never query account services.

Attached cards no longer show pin/close controls. Dragging beyond 8px pins the card at
its output-clamped position and reveals Close; Escape also dismisses it. Dragging uses
the stationary shell coordinate space and excludes controls, selectable text, and
scrolling content. Cross-output detachment/reattachment remains a later slice.
OpenRouter omits the empty token-list container so unavailable usage does not add a
trailing 10px gap.

### Network panel and platform boundary

`src/network.rs` defines toolkit-independent device, access-point, address, traffic,
capability, error, and action types plus an object-safe asynchronous `Adapter`.
`src/network/linux.rs` owns NetworkManager CLI arguments/parsing, optional resolved DNS
lookup, and Linux counters. Both the bar indicator and GTK panel consume this boundary.
Another platform implements the trait and selection; card widgets contain no tool names
or shell commands. This is a network boundary, not a claim that the whole Wayland
application already runs on another OS. Unsupported platforms fail explicitly. `Fake`
records credential-free action names and never probes the host.

The GTK network card retains connected Wi-Fi/Ethernet details, traffic since adapter
reset, addresses/default DNS, at most three complete nearby-network rows, scan/radio
controls, saved-credential or password connection, disconnect, and inactive Ethernet
state/connection. Passwords are cleared on submission, cancellation, target loss, and
unmapping; they travel to nmcli over stdin, never argv. Permission, timeout, unavailable
service, and stale-target results are sanitized. Busy controls reconcile with the
adapter response; background status cannot acknowledge an outstanding mutation.

One shared card/controller serves all bars in the process. Its bounded worker polls
status every 10 seconds and counters every second only while mapped; unmapping cancels
outstanding helper processes, drops pending requests, clears secrets, and invalidates
late replies. This slice uses polling rather than NetworkManager D-Bus subscriptions.
NetworkManager remains responsible for credentials and authorization;
enterprise/hidden-network setup is not a new workflow in this migration.

Validation commands:

- `dev-check python3 apps/bartjik/tests/gtk_panel_primitives.py --network` renders GTK
  and QML fixtures and exercises synthetic connect, radio, Ethernet, permission-error
  recovery, device loss, and password clearing.
- `dev-check python3 apps/bartjik/tests/network_adapter.py` substitutes private nmcli
  and resolvectl executables for protocol/mutation validation without host actions.
- Cargo unit tests cover fake adapter actions/failures, traffic counter resets, Linux
  parsing/error mapping, and worker teardown when the card closes.

Rendered artifacts: `/tmp/bartjik-network-primitives.png` and
`/tmp/bartjik-network-reference.png`. Physical Wi-Fi association, authorization-agent
prompts, and mixed-DPI/IME interaction still require device acceptance; automated checks
never disconnect the host network.

Validation passed: build, static checks, 55 unit tests, private-tool tests, and the
rendered interaction fixture. Read-only host verification found connected Wi-Fi and
unavailable Ethernet; no real connection/radio actions were executed. Installed
byte/rollback receipt: `/tmp/bartjik-gtk-network-install.json`.

### Network width regression

Live-sized traffic values reproduced a 414px minimum body inside the 388px content area,
clipping every right-edge control. Traffic columns now size independently, with
ellipsizing labels allowing constrained allocations instead of widening the card.
Connection subtitles also shrink safely. The rendered network fixture uses 138 GiB
transferred and multi-digit KiB/s rates, asserts the body minimum fits, and checks that
the Wi-Fi, disconnect, and Ethernet controls stay within its bounds.
