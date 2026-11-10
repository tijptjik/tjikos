# tjik-gtk

GTK4 components for Linux desktop applications: themed surfaces, virtualized result
lists and grids, asynchronous image loading, drag/resize controls and animation helpers.

Application state, commands and persistence stay in your application. Result adapters
accept ordinary GObject models, and desktop pointer integration is supplied through
callbacks rather than a compositor-specific dependency.

## Requirements

- Rust and GTK 4.12 or newer.
- GTK4 and gtk4-layer-shell development libraries, discoverable through `pkg-config`.
- A Wayland compositor with layer-shell support for layer surfaces. Ordinary GTK windows
  can use the presentation components without layer-shell surface behavior.

The Rust package links against these native libraries; it does not bundle them. Install
them through your distribution or configure `PKG_CONFIG_PATH` for a custom installation.
Keep GTK3 and GTK4 in separate processes when integrating with older software.

## Add the dependency

```toml
[dependencies]
tjik-gtk = "0.1"
```

Use the re-exported `tjik_gtk::gtk` types to match the library's GTK bindings.

## Create a result view

`results::factory` maps each GObject model item to a `Presentation`. Your adapter
supplies text, icons and status; the factory handles layout, escaping and widget
recycling.

```rust
use tjik_gtk::{
    gtk::{self, prelude::*},
    results::{self, Layout, Presentation},
};

fn main() {
    gtk::init().expect("GTK initialization failed");

    let factory = results::factory(
        Layout::List,
        40,
        |position| println!("Selected item {position}"),
        |object| {
            let title = object
                .downcast_ref::<gtk::StringObject>()
                .map(|item| item.string().to_string())
                .unwrap_or_default();
            Presentation {
                title,
                fallback_icon: "text-x-generic".into(),
                ..Default::default()
            }
        },
    );

    let model = gtk::StringList::new(&["First result", "Second result"]);
    let selection = gtk::SingleSelection::new(Some(model));
    let list = gtk::ListView::new(Some(selection), Some(factory));
    list.set_vexpand(true);
    // Attach `list` to your application's window or scrolled container.
}
```

The adapter callback runs on the GTK thread during binding. Keep blocking I/O and
expensive computations outside it. `scroll::by_row` provides row-based scrolling and
hover selection for factory-created rows without scanning the model.

## Theme your application

`theme::Theme` defines the palette. `theme::apply` installs the primitive stylesheet;
`theme::apply_with_css` appends your application CSS. The styles apply below a widget
with the `tjik` CSS class.

Application CSS can use `@tjik_background`, `@tjik_surface`, `@tjik_text`,
`@tjik_muted`, `@tjik_accent` and `@tjik_border`. Reapply the stylesheet when the
palette or font changes.

## Load images asynchronously

`images::set` displays a themed icon or loads a local/embedded image with a fallback.
`images::set_picture` provides the same loading behavior for `GtkPicture`. Remote URLs
are not fetched.

Each GTK thread runs one decode at a time. Waiting requests hold one weak reference per
live widget, with no copied source strings or textures. Rebinding coalesces requests,
dead targets are pruned, and results for a different widget binding are discarded.

Embedded images are limited to 128 KiB and local encoded files to 4 MiB. Output textures
preserve aspect ratio, do not enlarge small images and fit within 256 × 256 pixels. The
embedded-image cache holds at most 32 textures, up to 8 MiB of pixel storage. Local
files are not cached because their contents can change. Native decoders can allocate
additional intermediate memory; use thumbnail-sized sources for predictable resource
usage.

## Surfaces and animation

`surface` provides layer-shell setup, keyboard handoff, monitor placement and
drag/resize helpers. `draggable_with_position` accepts a global-pointer callback from
your desktop adapter. Set `TJIK_GTK_WINDOWED` to use an ordinary window instead of a
layer surface.

`motion::ClosingPulse` owns a pending-close animation. Retain the controller with your
view, call `set_pending` when state changes and drop it when the view is disposed. It
holds widgets weakly and cancels callbacks on unmap. The result factory manages this
lifetime for recycled rows automatically.

`motion` and `removal` effects respect GTK's animation preference. Use
`removal::animate` only for confirmed deletions, and cancel outstanding removal effects
when replacing a snapshot or hiding the view.

## License

Authored by **tijptjik** and licensed under [MPL-2.0](LICENSE). See [NOTICE](NOTICE) for
source notices. Third-party components retain their own licenses.
