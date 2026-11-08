//! Finite, frame-clock driven effects. No timers remain after completion or unmap.
use gtk::{glib, prelude::*};
use std::{cell::RefCell, rc::Rc};

pub fn enabled(widget: &impl IsA<gtk::Widget>) -> bool {
    widget.settings().is_gtk_enable_animations()
}

/// A subdued startup mark; animation runs only while the mark is mapped.
pub fn busy_mark(bytes: &'static [u8]) -> gtk::Picture {
    let texture =
        gtk::gdk::Texture::from_bytes(&glib::Bytes::from_static(bytes)).expect("bundled logo");
    let mark = gtk::Picture::for_paintable(&texture);
    mark.set_size_request(88, 88);
    mark.set_halign(gtk::Align::Center);
    mark.set_valign(gtk::Align::Center);
    mark.set_can_target(false);
    mark.set_opacity(0.28);
    let tick = Rc::new(RefCell::new(None::<gtk::TickCallbackId>));
    let active = tick.clone();
    mark.connect_map(move |mark| {
        if !enabled(mark) {
            return;
        }
        let start = RefCell::new(None);
        *active.borrow_mut() = Some(mark.add_tick_callback(move |mark, clock| {
            let start = *start.borrow_mut().get_or_insert(clock.frame_time());
            let phase = (clock.frame_time() - start) as f64 / 1_600_000. * std::f64::consts::TAU;
            mark.set_opacity(0.22 + 0.12 * (1. - phase.cos()) / 2.);
            glib::ControlFlow::Continue
        }));
    });
    mark.connect_unmap(move |mark| {
        if let Some(tick) = tick.borrow_mut().take() {
            tick.remove();
        }
        mark.set_opacity(0.28);
    });
    mark
}

/// Short feedback fades honor the desktop's reduced-motion preference.
pub fn reveal(widget: &impl IsA<gtk::Widget>) {
    if !enabled(widget) {
        return;
    }
    let start = RefCell::new(None);
    widget.set_opacity(0.);
    widget.add_tick_callback(move |widget, clock| {
        let start = *start.borrow_mut().get_or_insert(clock.frame_time());
        let progress = ((clock.frame_time() - start) as f64 / 120_000.).clamp(0., 1.);
        widget.set_opacity(progress);
        if progress >= 1. {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

/// Pulse only while the backend still reports an application as closing.
pub fn closing(row: &impl IsA<gtk::Widget>, pending: bool) {
    if !pending {
        row.remove_css_class("closing");
        row.set_opacity(1.);
        return;
    }
    if row.has_css_class("closing") || !enabled(row) {
        return;
    }
    row.add_css_class("closing");
    let start = RefCell::new(None);
    row.add_tick_callback(move |row, clock| {
        if !row.has_css_class("closing") {
            row.set_opacity(1.);
            return glib::ControlFlow::Break;
        }
        let start = *start.borrow_mut().get_or_insert(clock.frame_time());
        let phase = (clock.frame_time() - start) as f64 / 900_000. * std::f64::consts::TAU;
        row.set_opacity(0.65 + 0.35 * phase.cos());
        glib::ControlFlow::Continue
    });
}
