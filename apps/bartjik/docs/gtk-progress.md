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
