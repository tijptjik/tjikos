# Shared GTK presentation

`tjik-gtk` contains GTK4 styling, virtualized result factories, and layer-shell surface
behavior. It reads the existing Tjik theme tokens and font settings. Applications retain
their own commands, state, persistence and desktop-specific integration. GTK widgets
stay on the main thread; application I/O belongs on workers.

The layer uses a brief exclusive keyboard handoff followed by on-demand focus, without
reserving desktop space. Standalone consumers can set `TJIK_GTK_WINDOWED`. The launcher
shares drag, resize and monitor primitives without imposing Hyprland IPC on other
applications.

The native layer-shell library is built from the pinned upstream revision in
`scripts/native.sh`. Runtjik packages it in `lib/` and uses an executable-relative
RPATH, so installations do not depend on a developer's build directory. Build through
`dev-check bun run build --filter=@tjikos/runtjik` on managed desktops.

Keep backend GTK3 icon resolution in a separate process from GTK4. This avoids mixing
incompatible GTK versions and permits migration one frontend at a time.

`motion` owns mapped-only startup pulses, finite feedback fades and pending-close
pulses. `removal` captures only confirmed deletions into logical GTK render nodes, with
app shrink/fade and gallery fragment dissolve. Both honor GTK's animation setting.
Applications cancel removal overlays on new result snapshots and hide.
`draggable_with_position` accepts an optional desktop global-pointer adapter; this
avoids moving-surface coordinate feedback without coupling the shared crate to Hyprland.
