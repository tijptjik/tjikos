# Bartjik GTK migration and compositor adapter handoff

## Mission and authority

Replace Bartjik’s Quickshell frontend with Rust + GTK4, developing and validating on
**Hyprland first**. Preserve the current visual identity, interactions, system controls,
configuration, and operational safeguards. Make support for another Wayland compositor
an adapter implementation, not an application rewrite.

This is an implementation brief for a fresh coding agent. It specifies proposed
architecture; the proposed packages and adapters do not exist merely because they are
named here. This documentation task does not itself authorize starting the migration,
changing services, or switching the active bar. When the user assigns implementation,
work through the stages below and the repository’s current instructions.

Read `/home/io/code/tjikos/AGENTS.md` and any more specific instructions. Applications
live in `apps/`, shared packages in `packages/`; authoritative managed desktop
configuration is in `/home/io/code/dotjik/native/`. Read Dotjik’s instructions before
changing it. Preserve unrelated dirty work. Do not commit or push without authorization.

For implementation, build and validate each completed app change, then install using its
supported installer and verify the installed bytes. Run heavy checks through
`dev-check`; never bypass its resource lock. Place every visible test window, including
nested compositors and child dialogs, on **workspace 7 silently before launch**, without
switching the user’s workspace or taking focus. Use synthetic fixtures and private
buses. Preserve active captures, archive workers, credentials, histories, and user
configuration.

The first implementation milestone is a shared compositor boundary plus a small working
GTK Bartjik surface. Do not begin by rewriting every provider or creating a large plugin
framework.

## Current architecture and source map

The source audit behind this brief was performed on 2026-11-09. Recheck the working tree
and installed runtime before acting; this repository is developed concurrently.

Bartjik already has a Rust backend. Preserve and evolve it rather than rebuilding its
business logic in GTK callbacks:

- `apps/bartjik/src/engine.rs`, `providers.rs`, `actions.rs`, `contracts.rs`, and
  `contract.json`: provider scheduling, payloads, commands, and configuration contracts.
- `apps/bartjik/src/workspaces.rs`: Hyprland IPC, geometry, workspace/window logic.
- `apps/bartjik/src/{audio,network,connectivity,battery,idle,nightlight,monitors}.rs`:
  existing system integration and policy.
- `apps/bartjik/src/{tray,spotify,navigation,lyrics,hyprvoice,screen_record}.rs`:
  existing attention, media, navigation, and activity helpers. `tray.rs` consumes a
  StatusNotifierWatcher; that is not proof it implements a replacement watcher or full
  menu host.
- `apps/bartjik/qml/Native.qml`: Quickshell MPRIS and SystemTray objects, native
  rows/actions, and shared helper lifecycles.
- `AudioDetails.qml`, `AudioGroup.qml`: direct Quickshell PipeWire objects and
  mutations, plus backend helpers for routing and meters.
- `Bar.qml`, `Reserve.qml`, `PanelShell.qml`, `StablePanelSurface.qml`,
  `DetachablePopup.qml`, `DetachedWindow.qml`, `DetailPanel.qml`: bar reservation, panel
  geometry, input masks, keyboard ownership, pinning, dragging, detaching, and animated
  handoffs.
- `WorkspacePreview.qml`: Hyprland toplevel objects and Quickshell `ScreencopyView`,
  including on-demand frame capture. This is a significant dependency, not just a grid
  of labels.
- `TrayMenu.qml`: `QsMenuOpener`, nested menus, themed interaction states, and popup
  placement without a grabbing input serial.
- `NotificationHistory.qml` and the `qml/Notifications` symlink to
  `packages/notification-history/qml`: shared notification presentation, including
  Telegram playback/recording components.
- `apps/bartjik/docs/{inventory,verification,rust-migration}.md`, themes, and existing
  screenshot artifacts: starting inventories and design references, not evidence that a
  new implementation has passed.

### Runtjik is only partially isolated today

`apps/runtjik/src/gtk/desktop.rs` gathers several Hyprland operations, and
`packages/tjik-gtk/src/surface.rs` contains reusable GTK/layer-shell mechanics. This is
a useful seam, **not a complete compositor adapter**.

Hyprland assumptions also remain in
`apps/runtjik/src/{launcher,windows,close,capture,capture_worker,image_viewer,browser_host}.rs`,
the GTK outside-click Lua hook, and comparison/testing scripts. They include window
addresses, monitor discovery, focus dispatch, special workspaces, pointer coordinates,
capture targeting, and placement rules. Some GTK desktop helpers still execute
synchronous compositor queries. Moving only `desktop.rs` would leave both portability
and UI-latency problems unresolved.

Extract these dependencies incrementally into the same boundary Bartjik will use. Keep
Runtjik working on Hyprland throughout. Do not require a second production compositor
before the first adapter is useful; do require a fake adapter proving that application
code no longer depends on Hyprland’s schema.

## Architecture to implement

Use four distinct layers:

```text
Bartjik GTK / Runtjik GTK
    | presentation state, typed user intents, capabilities
    v
Application controllers and existing domain cores
    |                         |
    v                         v
Compositor contracts          Desktop service contracts
    |                         |-- tray / DBusMenu
    |-- Hyprland adapter      |-- audio / media / Bluetooth
    |-- fake adapter          |-- network / power / Notjik
    |-- future Niri adapter   |-- existing app-specific providers
    v
IPC, negotiated Wayland protocols, portals, D-Bus, bounded helpers

Shared GTK widgets and surfaces provide rendering mechanics, not host policy.
```

### Package boundaries

The following names are suggestions; choose a small coherent initial set and document
the actual names:

| Package or module                             | Responsibility                                                                                | Must not contain                                              |
| --------------------------------------------- | --------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| `tjik-desktop`                                | Toolkit-independent IDs, models, capabilities, events, errors, and compositor interfaces      | GTK widgets, Hyprland JSON, personal monitor names            |
| `tjik-desktop-hyprland`                       | Hyprland IPC, Lua/legacy version handling, event reconciliation, supported compositor actions | Bar cards, notification history storage, audio routing policy |
| `tjik-desktop-fake`                           | Deterministic synthetic topology, failures, events, and recorded commands                     | Access to the host session                                    |
| `tjik-wayland` or equivalent transport module | Reusable protocol discovery, capture transport, and protocol-level utilities where needed     | Claims that every compositor supports those protocols         |
| `tjik-desktop-services` initially             | Independent provider modules for SNI, DBusMenu, MPRIS, audio, BlueZ, power, etc.              | A giant compositor-specific service singleton                 |
| Existing `tjik-gtk`                           | Theme, icons, menus, widgets, animation, layer-shell and ordinary-window mechanics            | `hyprctl`, compositor environment detection, host policy      |
| Existing Bartjik core + new GTK frontend      | Bar composition, provider selection, controller state, rendering                              | Raw system commands scattered through widget callbacks        |

Split services into separate crates when reuse, dependencies, or lifecycle warrants it.
Start with statically linked Rust modules/traits and test doubles. A dynamic library
plugin ABI and a new universal daemon are unnecessary. Reuse one provider instance
across all bars in a Bartjik process; do not create one PipeWire connection or watcher
per monitor. Introduce cross-process shared services only where there is a demonstrated
ownership or duplication problem.

### Compositor contract

Prefer focused interfaces over one enormous trait. An illustrative contract, not code to
paste verbatim:

```rust
trait DesktopSession {
    fn capabilities(&self) -> DesktopCapabilities;
    fn events(&self) -> EventStream;
    async fn snapshot(&self) -> Result<DesktopSnapshot, DesktopError>;
}
trait WindowActions {
    async fn activate(&self, target: WindowId, context: ActivationContext)
        -> Result<ActionReceipt, DesktopError>;
    async fn close(&self, target: WindowId) -> Result<ActionReceipt, DesktopError>;
}
trait WorkspaceActions { /* select, move, reveal, supported extensions */ }
trait OutputControl { /* inspect, propose, apply, confirm, rollback */ }
```

Use the project’s chosen object-safe async pattern; decide it during the first slice. Do
not accidentally require GTK objects to be `Send` or expose a runtime-specific stream to
every consumer.

Define these semantics before moving implementations:

- Opaque window/workspace/output identifiers, scoped to a compositor session generation.
  A Hyprland address and a Niri numeric ID are not interchangeable strings.
- Normalized title, app ID/desktop-entry hints, focus state, output association,
  workspace membership, and optional geometry. Unknown/unavailable is different from an
  empty value.
- Logical coordinates, physical buffer dimensions, scale, transform, and coordinate
  space carried explicitly. Output identities must not depend on monitor list indices.
- Snapshot revision plus ordered events; initial synchronization, reconnect,
  resubscription, missed-event recovery, and invalidation on compositor restart.
- Explicit `Unsupported`, `Unavailable`, `PermissionDenied`, `Cancelled`, `StaleTarget`,
  and timeout results. No successful no-op masquerading as an implemented action.
- Window activation as an intent. The adapter owns workspace revelation, monitor
  selection, focus handoff, and compositor-specific ordering. Revalidate the target
  after asynchronous work. Honor activation-token/input-serial requirements rather than
  inventing user interaction.
- Output reconfiguration as a transaction with the existing confirmation timeout and
  rollback, including frontend crashes or disconnected displays.
- Outside-click observation, global pointer access, special workspaces, absolute
  positioning, desktop/game mode, workspace-scoped recording, and live window thumbnails
  as optional capabilities. Do not bake them into mandatory Wayland semantics.
- Keep generic launch behavior separate from focus/activation. Keep capture transport
  separate from the application’s recording lifecycle and file/history policy.

Widgets must not branch on compositor names. Controllers use capability results;
adapters own mappings and compositor-specific extension data. Where semantics genuinely
differ, expose a well-defined optional feature rather than pretending equivalence.

### Runtime selection and diagnostics

Prefer one `runtjik` and one `bartjik` binary with built-in adapters. Optional Cargo
features may reduce dependencies; separate `runtjik-hyprland` and `runtjik-niri`
application forks are not the default design.

Resolve the session once and pass its identity to worker processes:

1. Accept an explicit supported `--compositor`/configuration override, including a
   deterministic test choice.
2. Use session environment as hints, then probe the corresponding endpoint with a
   bounded, read-only identity/capability handshake. `HYPRLAND_INSTANCE_SIGNATURE`,
   `NIRI_SOCKET`, and desktop-name variables alone are insufficient evidence.
3. In nested sessions, bind to the selected Wayland display and runtime/IPC endpoints.
   Never probe the host and silently attach to it when the private compositor lacks a
   feature.
4. If detection is ambiguous, report candidates and require an explicit override; do not
   guess by process-list order. If unavailable, provide the documented reduced mode
   without silently routing actions elsewhere.
5. Expose selected adapter, versions, endpoints in privacy-safe form, capabilities, and
   degraded features through `status`/`doctor`. Distinguish build support, advertised
   protocol support, permission, and working runtime availability.

Layer shell is an optional Wayland extension, not core Wayland. GTK’s ordinary-window
fallback can keep a launcher usable; it does not make an unsupported desktop bar
equivalent to a correctly reserved layer surface.

## Replace Quickshell integration deliberately

Inventory all imports, native object uses, QML actions, implicit Qt behavior, and
helpers. For each, record the current owner, replacement, state/event contract, mutation
path, fallback, and validation evidence.

| Integration                    | Replacement direction                                                                                                   | Required coverage                                                                                                                                                         |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Bar and panel surfaces         | GTK4 + the existing pinned/bundled layer-shell approach; compositor adapter for nonportable rules                       | Multi-output bars, edge reservation, auto-hide, empty input regions, popup anchoring, pointer corridors, keyboard ownership, detachment/reattachment, cross-output motion |
| SNI tray                       | Rust D-Bus host plus explicit watcher ownership strategy                                                                | Item registration by service or object path, owner changes, restarts, icons/pixmaps, attention, tooltips, primary/secondary/context/scroll actions                        |
| Tray menus                     | `com.canonical.dbusmenu` client with a themed GTK menu model                                                            | `GetLayout`, revisions and updates, `AboutToShow`, events, separators, radio/check states, disabled/invisible items, nested submenus, item removal during interaction     |
| Media                          | MPRIS D-Bus provider; reuse Spotify/navigation/lyrics logic                                                             | Player selection, metadata, capabilities, position/seek, play/pause, identity changes, artwork, unavailable players; no always-running position timer for hidden UI       |
| Audio                          | Replace Quickshell PipeWire object subscriptions with a tested native provider                                          | Outputs, inputs, per-client streams, volume/mute, default route, stream moves, disappearing IDs, policy confirmation, metering only while relevant UI is visible          |
| Notifications                  | Keep Notjik as authority; port Bartjik history presentation and its client integration                                  | DND, unread state, history updates, dismissal/actions, grouping, locked privacy, attachments, Telegram playback/recording boundaries                                      |
| Network/VPN                    | Preserve existing Rust helpers initially; prefer NetworkManager D-Bus for native state/actions when replacing transport | Wi-Fi scan/connect/disconnect, failure states, secrets/authorization, addresses/traffic, Mullvad and Tailscale controls                                                   |
| Bluetooth                      | Add an independent BlueZ D-Bus provider when required by the inventory/user scope                                       | Adapter power, discovery lifecycle, pairing/agent prompts, trust/connect/disconnect, battery when supported, rfkill and permission failures                               |
| Battery/power                  | Reuse existing backend and system D-Bus services such as UPower, power-profiles-daemon, logind                          | System versus peripheral batteries, transitions, absent services, power-profile authorization, confirmation for destructive/session actions                               |
| Idle/nightlight/monitor layout | Preserve existing persistent policy; isolate Hyprland-specific executors                                                | Expiring inhibition, service restarts, scheduler independence, output change preview/rollback, unsupported controls on other compositors                                  |
| Workspace/window previews      | Adapter-owned discovery plus capability-selected capture transport                                                      | True windows and geometry, minimized/hidden/private cases, frame lifetime, bounded refresh while visible, multi-DPI; explicit fallback if live capture is unavailable     |
| Icons/fonts/clipboard/IPC      | GTK/GIO and explicit Rust contracts; reuse existing artwork and clipboard authorities                                   | Icon names/files/data URIs, SVG and pixmaps, fallback and caching, menu typography, selections, control socket identity and reconnect                                     |
| Other providers                | Retain current Rust implementations and configuration                                                                   | datetime/locale, Codex/OpenRouter quotas, crypto, custom process modules, hyprvoice, recording indicators, and every enabled module in `contract.json`                    |

Bluetooth was **not** listed as a provider in the inspected Bartjik contract, and the
inspected sources did not reveal a dedicated Bluetooth module. Do not present it as
existing parity already implemented. Check whether current access comes through a tray
application or external launcher; then record whether native Bluetooth is new scope or a
replacement of that workflow.

### Notification ownership is not bar ownership

`apps/notjik/qml/shell.qml` currently contains a Quickshell `NotificationServer`.
Bartjik consumes Notjik status/actions and shared notification-history UI. Removing
Quickshell from Bartjik does not, by itself, remove it from Notjik or the rest of the
desktop.

Do not have Bartjik claim `org.freedesktop.Notifications`. Preserve a single
notification authority, IDs, actions, dismissal semantics, history, and privacy. Port
shared history presentation into a reusable GTK package where appropriate. Audit the
shared QML media components too: audio playback/recording is not supplied merely by
replacing labels. A future Notjik server migration is a separate task with an explicit
ownership handoff.

### Tray ownership and real service lifecycles

Determine which process currently owns `org.kde.StatusNotifierWatcher`, whether other
hosts are registered, and what disappears when Quickshell exits. Reuse an existing valid
watcher when available; otherwise implement or deliberately adopt one. Handle startup
order and owner loss. A read-only roster consumer cannot replace the watcher.

SNI pixmaps require correct dimensions, channel order, stride, scaling, symbolic
styling, overlay/attention icons, and fallbacks. Keep stable item identity separate from
labels and transient bus owners. Do not implement a tray as a fixed collection of launch
buttons or claim parity from three mocked icons.

Quickshell’s menu handles are not `GMenuModel` objects that GTK automatically
understands. Implement the DBusMenu translation and lifecycle explicitly. Start with
synthetic nested-menu fixtures on a private bus, then validate actual applications
without sending unintended actions.

### Audio and other device providers

A normal PipeWire desktop often exposes PulseAudio compatibility APIs, but their graph
and routing semantics are not identical to native PipeWire/WirePlumber. Choose the
smallest backend that demonstrably covers the existing controls. Existing
`pactl`/`wpctl` or other helpers can remain behind a provider during migration; label
that transport honestly and measure its cost. Do not make a widget spawn a process per
slider tick.

Use Gio D-Bus or another justified Rust D-Bus library consistently; avoid multiple
competing connection stacks without a reason. Move subscriptions and blocking helpers
off the GTK thread. Use typed snapshots, bounded/coalesced updates, debounced mutations,
cancellation, and explicit command acknowledgements. Optimistic UI must reconcile with
authoritative device state.

Keep hardware names, preferred audio routes, monitor connectors, and per-host behavior
in configuration/Dotjik, not shared Rust widgets. Handle missing daemons, disabled
radios, device removal, bus restarts, and denied authorization as normal states. Respect
existing polkit/secret agents; do not introduce root GUI execution or silently take over
an agent role.

## Migration stages and gates

### 1. Establish the baseline

Identify the actual service command, PID, installed/source paths, configuration, package
versions, and bus-name owners. Read current docs and tests, not historical paths copied
from this brief’s references. Produce a dependency and parity matrix covering every
configured provider, panel, action, animation, menu, and source of state.

Capture synthetic reference images at fixed dimensions, font/theme, output scale, and
seeded data. Record startup readiness, idle CPU/wakeups, total frontend-plus-backend
PSS, open-panel cost, pointer/keyboard responsiveness, and active-preview/meter costs.
Report hashes and environment. Do not treat Runtjik’s earlier benchmark numbers as
Bartjik evidence.

### 2. Establish the shared adapter seam on Hyprland

Implement normalized models, capabilities, Hyprland transport, and a fake adapter. Move
one vertical slice through the boundary: output/workspace discovery and activate an
existing window. Cover Runtjik’s reveal/focus handoff and Bartjik’s workspace action
with the same contract tests.

Then move the remaining direct compositor calls incrementally. Add a narrowly scoped
dependency check to keep `hyprctl`, Hyprland socket names/JSON parsing, and `hl.*`
dispatch strings out of production widget/controller code. Explicitly allow the adapter,
its fixtures, and compositor-specific test harnesses. Leave domain storage and provider
policy in their existing owners.

### 3. Build the GTK surface shell

Add an opt-in GTK frontend and retain the Quickshell fallback. Implement bars, islands,
dimensions, reservation, hover/pin state, tooltips, panel placement, focus, and input
masks before filling every card. Reuse theme tokens and reviewed artwork.

Port transitions, detachment, drag-from-body behavior, and cross-output placement as
behavior, not approximate screenshots. Controls must retain their own gestures; dragging
must not hijack sliders, buttons, text selection, or scroll areas. A hidden fullscreen
layer must not intercept desktop input. Preserve synchronized versus independent
auto-hide behavior per configuration.

### 4. Port service integrations and cards in small slices

Start with simple existing backend providers, then MPRIS, audio, SNI/DBusMenu,
notification history, connectivity, monitor/power/idle panels, and previews. Adjust
sequence according to dependencies, not just ease of screenshots.

For each slice: implement read state, mutations, pending/error states,
reconnect/teardown, accessibility/keyboard behavior, visuals, and tests together. Remove
the corresponding Quickshell dependency only after its replacement works. Do not rewrite
Notjik, standalone Tasktjik, account providers, or fleet policy as incidental work.

### 5. Compare and measure

Provide an explicit comparison mode and documented close/status commands. Use private
configuration/runtime directories, synthetic providers, and a private D-Bus session or
fixtures. Both frontends must be interactive, correctly placed on workspace 7, and use
equivalent theme/geometry/data.

Two bars must not both reserve the real desktop edge, claim singleton bus names, or
execute duplicate system mutations. In safe comparison mode, actions go to fixture
providers. Any later real-device verification must have one mutation authority. Never
let a demo’s missing service fall back to the user’s real bus, compositor, clipboard, or
account.

Measure the renderer and provider processes together, including persistent helpers. Test
both appropriate GPU rendering and software fallback; select based on measured workload
rather than assuming GPU is always faster. Distinguish process-cold,
application-cache-cold, and OS-cache-cold runs; do not flush global caches or delete
histories to manufacture a cold result. No forced production startup delay just to
display an animation.

### 6. Install, verify, and switch deliberately

Use the supported Bartjik build/install path, updating it for the GTK artifact and
bundled native dependencies. Preserve user configuration, persistent idle/nightlight
policy, service identities, and active workers. Make binary/resource replacement
coherent; keep a rollback route. Do not blindly copy Runtjik’s installer over Bartjik’s
distinct service setup.

Before switching the default, demonstrate the parity matrix, known unsupported
capabilities, rendered comparisons, performance results, and recovery tests. An opt-in
installed build is not a default switch. Once switching is authorized, update both the
installed launcher/service and authoritative Dotjik policy, restart only the identified
Bartjik process, verify the running binary and installed hashes, and test the reopened
UI. Keep an explicit legacy option until the agreed acceptance gate is met.

## Acceptance checklist

The implementation is not complete just because it compiles or a shell appears.

- Every current configured module and interaction is mapped to a replacement or an
  explicitly accepted gap. Bluetooth/new features are distinguished from parity.
- UI code has no compositor-specific IPC or dispatch. Runtjik and Bartjik use the same
  selected adapter contract. A fake compositor with non-Hyprland IDs and missing
  capabilities runs both controller paths.
- Snapshot/event replay covers reconnects, compositor restart, output hotplug/rotation,
  changing focus/workspaces, stale targets, and backend failures. Commands cannot act on
  a reused ID from a previous session.
- Tray fixtures cover watcher loss, owner changes, menus with updates and
  nested/check/radio/disabled entries, icon formats, attention, and item deletion while
  a menu is open.
- Audio/media/network/power providers cover missing and restarted services,
  authorization errors, rapid updates, and device removal. Hidden panels stop meters and
  capture streams; there are no unbounded queues or child-process leaks.
- Notification actions still reach Notjik, locked history stays private, and no second
  notification server is introduced.
- Real input tests cover hover corridors, tooltips, scroll units and limits,
  drag/detach/reattach, pin/dismiss, popup keyboard focus, Escape, and IME in editable
  controls. Native menus do not unexpectedly grab or lose focus.
- Rendered evidence covers horizontal and vertical bars, constrained sizes, multiple
  displays, 1×/1.5×/2× scales, rotated outputs, light/dark variants where supported, and
  all important error/empty/loading states. Synthetic geometry tests do not substitute
  for physical mixed-DPI and IME-popup checks.
- Original icons, spacings, border/track treatment, aligned icon/text slots, headings,
  grouping, and animations are reviewed. Generic GTK checkboxes/cards are not accepted
  as substitutes for the original designed controls without an explicit design decision.
- Cold/warm startup, idle memory/wakeups, interactive latency, and active media/preview
  workload are measured with reproducible artifacts. Agree regression limits from the
  baseline before optimizing. Record unresolved limitations rather than claiming full
  parity.
- Build/static checks, focused tests, affected regression tests, installed byte
  verification, live process verification, and rollback instructions are recorded. No
  unrelated files or user data were discarded.

Existing starting commands, to recheck against current package scripts:

```sh
dev-check bun run build --filter=@tjikos/bartjik
dev-check bun run check --filter=@tjikos/bartjik
dev-check bun run --filter @tjikos/bartjik test
```

Existing renderer fixtures under `apps/bartjik/tests/` include `native_contracts.py`,
`tray_layout.py`, `workspace_ring.py`, `hyprvoice_render.py`, and focused audio, media,
monitor, network, history, attention, pointer, and performance tests. Audit visible-test
placement before running them; older fixtures may predate the workspace-7 rule. Add GTK
equivalents instead of merely keeping QML tests green. Reuse the isolation patterns in
`apps/runtjik/tests/{gtk_ui,gtk_desktop}.py` and the Runtjik compositor fixtures, not
their hard-coded app assumptions.

## Extending beyond Hyprland

A second compositor is a proof of the boundary, not a reason to promise universal
desktop-shell support.

| Target or feature set                                                | Expected effort after extraction                                                                       | Important limits                                                                                                                                                       |
| -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Basic Runtjik on another layer-shell compositor                      | Moderate: backend detection, outputs, window/workspace state, activation, close, and integration tests | Existing protocol support must be verified; activation/focus semantics differ                                                                                          |
| Niri window/workspace integration                                    | A natural next adapter using Niri’s own IPC/events                                                     | Dynamic workspaces, scrolling layouts, IDs, placement, and handoffs need native mappings; do not translate Hyprland special-workspace commands mechanically            |
| Sway integration                                                     | Another separate IPC implementation with common domain models                                          | Tree/output/workspace semantics and event lifecycle differ                                                                                                             |
| Full Bartjik parity on another compositor                            | Substantial beyond the basic adapter                                                                   | Live per-window previews, output transactions, outside-click observation, absolute pointer/placement, idle/nightlight, and workspace recording are the expensive edges |
| GNOME/KDE or other environments without the required shell protocols | Potentially a distinct shell integration/extension plus a reduced ordinary-window launcher             | A GTK window cannot replace compositor-reserved shell surfaces just by selecting another IPC socket                                                                    |

Check current protocol/API documentation and runtime advertisements when implementing
each adapter; this brief does not assert a particular Niri/Sway/GNOME/KDE version
supports every required feature. Standard D-Bus services—audio, BlueZ, MPRIS,
NetworkManager, UPower, SNI—are mostly independent of the compositor and should require
little or no UI change when their services are present.

Portals are useful for permission-mediated screenshot/screencast and selected
integrations, but are not a universal replacement for global window enumeration,
arbitrary placement, passive outside-click monitoring, or unrestricted live per-window
thumbnails. A screencast permission prompt and persistent session lifecycle must not be
hidden behind an apparently equivalent preview capability.

The architectural completion criterion is concrete: **adding Niri should add an adapter,
capability mappings, configuration/detection, and integration tests—not edits throughout
bar cards, launcher search, notification presentation, audio widgets, or storage code.**
Where the compositor cannot provide a feature, expose a truthful reduced mode.

## First work request for the implementing agent

Begin with a read-only live/source inventory and a proposed parity matrix. Then
implement the Hyprland/fake adapter seam and the output/workspace/window-activation
slice shared by Runtjik and Bartjik, followed by an opt-in GTK bar with correct
reservation and panel input behavior. Build, test, install, and provide a safe
interactive comparison. Keep the current bar as default until the later acceptance gate.
Maintain a short progress record with completed slices, exact commands/results,
artifacts, known gaps, and the next task so another agent can resume without
reconstructing history.
