use gtk::{gdk, prelude::*};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};

/// A centered, non-exclusive layer. Normal windows remain available for standalone apps.
pub fn launcher(window: &gtk::ApplicationWindow, namespace: &str) -> bool {
    if std::env::var_os("TJIK_GTK_WINDOWED").is_some() || !gtk4_layer_shell::is_supported() {
        return false;
    }
    window.init_layer_shell();
    window.set_namespace(Some(namespace));
    window.set_layer(Layer::Top);
    window.set_exclusive_zone(-1);
    keyboard(window, true);
    true
}
pub fn keyboard(window: &gtk::ApplicationWindow, active: bool) {
    if window.is_layer_window() {
        window.set_keyboard_mode(if active {
            KeyboardMode::Exclusive
        } else {
            KeyboardMode::None
        });
        if active {
            let weak = window.downgrade();
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
                if let Some(window) = weak.upgrade() {
                    if window.keyboard_mode() == KeyboardMode::Exclusive {
                        window.set_keyboard_mode(KeyboardMode::OnDemand);
                    }
                }
            });
        }
    }
}
/// Clear drag placement so the compositor centers the layer at its current size.
pub fn center(window: &gtk::ApplicationWindow) {
    if !window.is_layer_window() {
        return;
    }
    for edge in [
        gtk4_layer_shell::Edge::Left,
        gtk4_layer_shell::Edge::Right,
        gtk4_layer_shell::Edge::Top,
        gtk4_layer_shell::Edge::Bottom,
    ] {
        window.set_anchor(edge, false);
        window.set_margin(edge, 0);
    }
}
pub fn monitor(window: &gtk::ApplicationWindow, name: &str) {
    if !window.is_layer_window() {
        return;
    }
    let Some(display) = gdk::Display::default() else {
        return;
    };
    for object in display.monitors().iter::<gdk::Monitor>().flatten() {
        if object.connector().as_deref() == Some(name) {
            window.set_monitor(Some(&object));
            return;
        }
    }
}

/// Move a centered layer without reserving desktop space. Coordinates remain
/// compositor logical pixels, so pointer deltas also work on scaled outputs.
pub fn draggable(
    window: &gtk::ApplicationWindow,
    handle: &impl IsA<gtk::Widget>,
) -> gtk::GestureDrag {
    draggable_with_position(window, handle, || None)
}

/// Wayland has no global pointer API. Desktop adapters can supply logical global
/// coordinates to avoid feedback when the layer itself moves during a grab.
pub fn draggable_with_position(
    window: &gtk::ApplicationWindow,
    handle: &impl IsA<gtk::Widget>,
    position: impl Fn() -> Option<(f64, f64)> + 'static,
) -> gtk::GestureDrag {
    draggable_with_async_position(window, handle, move |done| done(position()))
}

/// Samples arrive on the GTK thread. Superseded gestures cannot move the surface.
pub fn draggable_with_async_position(
    window: &gtk::ApplicationWindow,
    handle: &impl IsA<gtk::Widget>,
    position: impl Fn(Box<dyn FnOnce(Option<(f64, f64)>)>) + 'static,
) -> gtk::GestureDrag {
    let position = std::rc::Rc::new(position);
    let pointer_origin = std::rc::Rc::new(std::cell::Cell::new(None));
    let drag = gtk::GestureDrag::new();
    let origin = std::rc::Rc::new(std::cell::Cell::new((0, 0)));
    let weak = window.downgrade();
    let start = origin.clone();
    let initial = pointer_origin.clone();
    let epoch = std::rc::Rc::new(std::cell::Cell::new(0_u64));
    let begin_epoch = epoch.clone();
    drag.connect_drag_begin(move |gesture, x, y| {
        if let Some(window) = weak.upgrade() {
            if let Some(handle) = gesture.widget() {
                let mut picked = handle.pick(x, y, gtk::PickFlags::DEFAULT);
                while let Some(widget) = picked {
                    if widget.is::<gtk::Entry>()
                        || widget.is::<gtk::Text>()
                        || widget.is::<gtk::Button>()
                        || widget.has_css_class("result")
                    {
                        gesture.set_state(gtk::EventSequenceState::Denied);
                        return;
                    }
                    picked = widget.parent();
                }
                if !window.is_layer_window() {
                    if let (Some(surface), Some(event), Some(point)) = (
                        window.surface().and_downcast::<gdk::Toplevel>(),
                        gesture.current_event(),
                        handle
                            .compute_point(&window, &gtk::graphene::Point::new(x as f32, y as f32)),
                    ) {
                        if let Some(device) = event.device() {
                            surface.begin_move(
                                &device,
                                1,
                                point.x() as f64,
                                point.y() as f64,
                                event.time(),
                            );
                        }
                    }
                    return;
                }
            }
            begin_epoch.set(begin_epoch.get().wrapping_add(1));
            initial.set(None);
            if !window.is_anchor(gtk4_layer_shell::Edge::Left) {
                offset(&window, 0, 0);
            }
            start.set((
                window.margin(gtk4_layer_shell::Edge::Left),
                window.margin(gtk4_layer_shell::Edge::Top),
            ));
        }
    });
    let end_epoch = epoch.clone();
    drag.connect_drag_end(move |_, _, _| end_epoch.set(end_epoch.get().wrapping_add(1)));
    let weak = window.downgrade();
    let sequence = std::rc::Rc::new(std::cell::Cell::new(0_u64));
    drag.connect_drag_update(move |_, x, y| {
        let current_epoch = epoch.get();
        sequence.set(sequence.get().wrapping_add(1));
        let current_sequence = sequence.get();
        let (epoch, sequence, initial, origin, weak) = (
            epoch.clone(),
            sequence.clone(),
            pointer_origin.clone(),
            origin.clone(),
            weak.clone(),
        );
        position(Box::new(move |point| {
            if epoch.get() != current_epoch || sequence.get() != current_sequence {
                return;
            }
            let (x, y) = if let Some(now) = point {
                // Until the first movement the surface has not moved; derive the
                // pointer origin from the event delta, avoiding an async press race.
                let start = initial.get().unwrap_or((now.0 - x, now.1 - y));
                initial.set(Some(start));
                (now.0 - start.0, now.1 - start.1)
            } else {
                (x, y)
            };
            if let Some(window) = weak.upgrade() {
                if window.is_layer_window() {
                    window.set_margin(gtk4_layer_shell::Edge::Left, origin.get().0 + x as i32);
                    window.set_margin(gtk4_layer_shell::Edge::Top, origin.get().1 + y as i32);
                }
            }
        }));
    });
    handle.add_controller(drag.clone());
    drag
}

pub fn offset(window: &gtk::ApplicationWindow, x: i32, y: i32) {
    if !window.is_layer_window() {
        return;
    }
    let monitor = window.monitor().or_else(|| {
        window
            .surface()
            .and_then(|surface| surface.display().monitor_at_surface(&surface))
    });
    let Some(monitor) = monitor else { return };
    let bounds = monitor.geometry();
    window.set_anchor(gtk4_layer_shell::Edge::Left, true);
    window.set_anchor(gtk4_layer_shell::Edge::Top, true);
    window.set_margin(
        gtk4_layer_shell::Edge::Left,
        ((bounds.width() - window.width()) / 2 + x).max(0),
    );
    window.set_margin(
        gtk4_layer_shell::Edge::Top,
        ((bounds.height() - window.height()) / 2 + y).max(0),
    );
}

pub fn resizable(window: &gtk::ApplicationWindow, handle: &impl IsA<gtk::Widget>) {
    handle.set_cursor_from_name(Some("se-resize"));
    let drag = gtk::GestureDrag::new();
    let origin = std::rc::Rc::new(std::cell::Cell::new((720, 600)));
    let weak = window.downgrade();
    let start = origin.clone();
    drag.connect_drag_begin(move |_, _, _| {
        if let Some(window) = weak.upgrade() {
            start.set((window.width(), window.height()));
        }
    });
    let weak = window.downgrade();
    drag.connect_drag_update(move |_, x, y| {
        if let Some(window) = weak.upgrade() {
            window.set_default_size(
                (origin.get().0 + x as i32).clamp(360, 1600),
                (origin.get().1 + y as i32).clamp(300, 1200),
            );
        }
    });
    handle.add_controller(drag);
}

/// The original Quickshell footer: five 2px rounded bars, heights 3/5/7/5/3.
/// Drawing geometry avoids font-dependent substitutions for the texture.
pub fn resize_grip() -> gtk::DrawingArea {
    let grip = gtk::DrawingArea::builder()
        .content_width(38)
        .content_height(12)
        .build();
    grip.set_draw_func(|widget, cr, width, height| {
        let color = widget.color();
        cr.set_source_rgba(
            color.red() as f64,
            color.green() as f64,
            color.blue() as f64,
            color.alpha() as f64 * 0.35,
        );
        cr.set_line_width(2.);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        for (index, bar_height) in [3., 5., 7., 5., 3.].into_iter().enumerate() {
            let x = width as f64 / 2. - 8. + index as f64 * 4.;
            let y = height as f64 / 2.;
            cr.move_to(x, y - (bar_height - 2.) / 2.);
            cr.line_to(x, y + (bar_height - 2.) / 2.);
            let _ = cr.stroke();
        }
    });
    grip
}
