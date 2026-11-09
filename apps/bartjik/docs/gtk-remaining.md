# GTK migration: remaining work

GTK is now the default bar; Quickshell remains available for explicit rollback. The
recent work implements system policy cards, crypto/music/workspace cards, live capture,
compact overflow tooltips, and independently owned detached Bartjik windows. Compilation
and synthetic fixtures are not a full parity sign-off.

## Implementation gaps

- Notjik detached cards now convert to compositor-owned windows through the shared
  adapter. The synthetic native-window fixture covers workspace-7 placement, focus,
  draft identity and scoped dismissal; live workspace-transition and drag review remain.
- Drag-back reattachment is out of scope by user decision: detached panels do not need
  to dock back into the bar.
- The user accepted current cross-monitor movement and output removal behavior. Detached
  provider refresh still retains its original output context; revisit this only for a
  demonstrated monitor-specific correctness issue.
- There is one detached view per provider (Mullvad/Tailscale share one). Clicking that
  provider activates its existing window; simultaneous independent copies of the same
  provider are not implemented.
- Keep adapter limitations explicit: native pre-map placement currently requires the
  Hyprland Lua adapter, and live capture uses Hyprland's export protocol. Other adapters
  return Unsupported; there is no second production compositor implementation yet.

## Acceptance and verification

- Review the live multi-panel handoff, independent close/Escape behavior, workspace
  switching and tooltip size. Synthetic tests verify ownership and snapshot retention;
  they do not prove that every compositor frame sequence is visually seamless.
- Compare thumbnail detail against Qt on identical live content, especially physical
  1x/1.5x/2x and rotated outputs. Check GPU and software rendering, capture cost and
  refresh suspension while panels are off-workspace.
- Exercise real-player controls and lyric synchronization, device/bus restarts,
  permission failures, monitor confirmation/rollback and unplugged outputs. Private
  fixtures cover backend mutations without touching real devices.
- Complete the provider/interaction parity matrix, including keyboard focus, IME, menus,
  controls during dragging, constrained layouts and error/empty states. Shared Notjik
  history rendering is implemented; audit its remaining rich-media interactions rather
  than relying on earlier, superseded text-only history limitations.
- Repeat startup and idle CPU/wakeups/PSS measurements with the complete frontend and
  provider processes. Recheck active capture/media workloads after the panel changes.
- The default switch is authorized. See `gtk-cutover.md` for deployed state,
  verification, rollback and the remaining normal-desktop acceptance observations.

## Current verification evidence

The final implementation passed the Bartjik build/static checks, 38 GTK unit tests, the
workspace render fixture, and the native detached-window fixture. The latter checks
pre-map workspace-7 placement, no initial focus, geometry, independent panel ownership,
scoped close actions, and handoff snapshot cleanup. The desktop adapter suite passed 15
tests; Notjik passed 63 unit tests. Selected interactive/private tests are run
separately from ignored tests. Detailed commands and artifacts are recorded in
`gtk-progress.md`; artifacts under `/tmp` are ephemeral, not committed references.
