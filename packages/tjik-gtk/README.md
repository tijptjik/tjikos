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

`scroll::by_row` works with rows created by `results::factory`. Hover selection resolves
the current GTK list-item position directly through a weak reference; it does not scan
or parse the model, retain list items, or rely on stable row IDs after recycling.

`images::set` resolves themed icons immediately and decodes local/embedded images on a
worker. `images::set_picture` uses the same queue and limits for `GtkPicture`, leaving
unsupported sources empty. Each GTK thread owns a queue of at most 64 pending requests
and one in-flight decode. Recycling removes superseded queued requests; stale
completions cannot update a rebound image. Queue saturation discards the oldest pending
request and retains its fallback icon. No timers or network fetches are used.

Embedded source strings are limited to 128 KiB and local encoded files to 4 MiB. Decoded
textures preserve aspect ratio, never enlarge small images, and fit within 256×256
pixels. At most 32 embedded textures are cached (up to 8 MiB of pixel storage). Local
files are not cached because their contents can change at the same path. Platform image
decoders can use additional intermediate memory; this is not a sandbox for hostile
media. Applications should provide thumbnail-sized local sources.

`results::factory` accepts a callback from any GObject model item to `Presentation`. The
application owns provider naming, model serialization, asset paths, icon priority, and
status interpretation. The library owns escaping, widget layout, recycling, selection
mechanics, and image loading. The callback runs during bind on the main thread and must
not perform blocking I/O. A plain-string adapter is included in the API documentation;
Runtjik's JSON adapter lives in its application source.

The shared theme styles primitives only. Applications append their own view styles via
`theme::apply_with_css`; the extra CSS can reference validated named palette colors
`@tjik_background`, `@tjik_surface`, `@tjik_text`, `@tjik_muted`, `@tjik_accent`, and
`@tjik_border`. Runtjik owns word/capture styles, and `tasktjik-gtk::style::CSS` owns
task and reading styles. Reapply the composed stylesheet when theme settings change.

The current supported platform is Linux with GTK 4.12+ and gtk4-layer-shell development
libraries. Rust crate packaging does not bundle those native libraries. The repository's
native setup script supplies a pinned gtk4-layer-shell build for local validation.

`motion::ClosingPulse` owns the lifetime of a closing animation. Keep its controller
with the view, call `set_pending` on state changes, and drop it when the view is
disposed. It holds widgets weakly, cancels callbacks on unmap, and resumes pending work
on map. The result factory manages this ownership automatically for recycled rows. The
display regression can be run in this repository with
`dev-check python3 -B packages/tjik-gtk/scripts/test-motion.py`.

Repository maintainers can validate distribution boundaries with
`dev-check bash packages/tjik-gtk/scripts/test-packages.sh`. It builds local archives
for `tjik-desktop`, `tjik-gtk` and `tasktjik-gtk`, extracts them outside the checkout,
and runs their non-display tests and public API examples offline. The Tasktjik archive
uses the extracted GTK archive through a Cargo patch until that version is published.
This does not check registry name availability or replace the display regressions. No
packages are uploaded. Publish `tjik-gtk` before its dependent `tasktjik-gtk` crate.

## License

The Tjik-authored source in this package is licensed under MPL-2.0; see `LICENSE` and
`NOTICE`. Standard MPL secondary-license compatibility is retained, allowing use in
GPLv3 applications. Third-party components retain their own licenses and notices.
