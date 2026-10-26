# Verification and boundaries

Validated on 2026-10-25 with Quickshell 0.3.2 on the existing Hyprland/Wayland session.
This is build/integration evidence, not a completed desktop switch-over.

- 32 Python tests cover configuration and overrides, typed IDs, read-only quota SQL,
  primary/secondary window duration and capacity weighting, credit/activity fixtures,
  audio routes and failures, monitor-local workspaces, temporary compositor event-socket
  disconnect/reconnect, process timeouts/output bounds/stream restart, rate limiting,
  stale retention, deduplication, reload cancellation, panel anchors, detached GUI
  launches and mocked dangerous actions.
- 126 JavaScript layout/interaction assertions execute the actual `qml/Logic.js`.
  All sides, nine position/alignment combinations, scale 1/1.5/2, physical/percentage
  widths, offsets, clipping, stacking and the exact 1920/640/400 → x240 and 960px →
  x80 examples pass. Stable item models preserve delegate identity through reordering.
- Real QML mouse events passed on **Wayland** and Qt's offscreen platform: independent
  icon/text targets, left/right/middle, delayed single/double disambiguation, both
  wheel directions, debounced hover enter/leave and complete screen/item/anchor context.
  A separate HoverHandler avoids MouseArea hover tracking inconsistencies after wheel
  and synthesized double-click sequences.
- The private-bus native fixture verifies native MPRIS discovery after startup,
  metadata changes, play/pause/next/previous/Quit and disappearance. Three named
  StatusNotifier services verify exclusion, duplicate application IDs, independent
  activation/secondary activation, scrolling, dynamic icons, menu population/action
  dispatch, identity retention while a menu is open, and menu/item removal. Menus are rendered through QsMenuOpener in a Quickshell
  PopupWindow; no platform QMenu/layer-shell attachment is required.
- The render test captures both themes, arrays and real islands. It checks right
  alignment within the left column on the actual screen, invalid reload retention,
  hide/show, vertical/top surfaces and one shared edge reservation. No QML binding
  warnings/errors remain. The isolated bus intentionally has no desktop portal;
  its expected ServiceUnknown portal warning is harmless.
- The source inventory and installed Quickshell type declarations were inspected.
  No notjik implementation, active Waybar process, live tray watcher or notification
  server was replaced or restarted.

Workspace build/check/test passed via Bun/Turborepo. The selective root installer
was smoke-tested into a temporary destination: packaged CLI initialization and
validation expose all 16 modules, documentation and the user unit are installed, and
no other app is installed. This uses `BARTJIK_INSTALL_HOME`/`BARTJIK_INSTALL_CONFIG`
test destinations and does not overwrite the live home.

## Read-only live checks

All 14 helper/compositor integrations returned valid data: workspaces, crypto (three
currency items), network, Mullvad, Tailscale, audio, datetime, Fcitx locale, Codex
weekly quota, OpenRouter, notjik DND, caffeine, nightlight and monitor layout.
The live two-output snapshot had distinct active workspaces (6 on DP-1, 12 on DP-2)
while only one output held focus; each view exposed its own active workspace.
Only availability/counts and workspace routing were logged. No account labels,
private balances, credentials, SSIDs or other private provider payloads were saved.

MPRIS and tray actions were exercised only against synthetic private-bus services.
Workspace focus changes, audio route/volume changes, panel launches, VPN connections,
idle/nightlight toggles and monitor changes were mocked rather than applied to the
user's desktop. Suspension and poweroff were mocked exclusively. Real physical
hotplug and a second scaled laptop display were not exercised; scale/reconnect logic
is covered by deterministic tests.

## Remaining limits

- Existing audio/network panels are external applications with an above-anchor
  placement contract. Bartjik passes precise logical geometry but does not add
  side-aware panel layout or remove their old independent Waybar hover observer.
  New bar hover bindings open a pinned panel, which is closed explicitly.
- Overflow clips; it does not supply a scroller or automatic priority collapse.
  Unusually large horizontal custom content needs a matching `thickness` to preserve
  stack spacing. Vertical content is bounded by `verticalWidth`.
- Native tray lifetime behavior ultimately follows installed Quickshell. Named SNI
  services passed registration/removal tests. In this 0.3.2 build, a fixture registering
  several object paths on an unnamed unique bus connection remained in SystemTray's
  model after disconnect; that upstream registration form is a known limitation.
- Tray popups deliberately do not take an input grab: use Close, a menu action or
  another tray menu to dismiss them. This keeps them reachable after leaving an
  auto-hidden bar and permits keyboard/IPC-originated opening without a Wayland
  pointer serial. Nested menus use an explicit Back button.
- HTTP fetches run off the QML thread and have bounded timeouts. Removing a provider
  cancels its subscription immediately; an already-running Python HTTP thread can
  finish its bounded request before the process exits. Caches are in memory only.
- The build has not enabled bartjik for the desktop session. Use the documented
  deliberate switch-over and rollback after reviewing connector/routes configuration.


Panel controls: `tests/test_connectivity.py` covers validated live country/city and
peer selection, private-address clipboard stdin, failed commands, shared relay
caching and hover-owned external-panel cleanup. `tests/panel_controls.py` clicks
Mullvad, Tailscale and input controls using QtTest with synthetic data, recording
requests and checking their origin context without executing connection changes.
`tests/hover.py --connectivity-only` checks real mouse hover over network/audio
icons, values and padding, with exactly one anchored open request per dwell.
`tests/details.py` captures the shared network-style panel frame using demo data.
