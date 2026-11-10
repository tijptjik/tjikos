//! Finite, frame-clock driven effects. No timers remain after completion or unmap.
use gtk::{glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

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

/// Owns one closing pulse for a widget. Drop cancels animation and disconnects signals.
/// The widget is held weakly; unmap cancels the callback and map resumes pending work.
pub struct ClosingPulse {
    inner: Rc<PulseState>,
    signals: Vec<glib::SignalHandlerId>,
}
struct PulseState {
    widget: glib::WeakRef<gtk::Widget>,
    pending: Cell<bool>,
    tick: RefCell<Option<gtk::TickCallbackId>>,
}
impl PulseState {
    fn stop(&self) {
        if let Some(tick) = self.tick.borrow_mut().take() {
            tick.remove();
        }
        if let Some(widget) = self.widget.upgrade() {
            widget.set_opacity(1.);
        }
    }
    fn start(&self) {
        let Some(widget) = self.widget.upgrade() else {
            return;
        };
        if !self.pending.get()
            || self.tick.borrow().is_some()
            || !widget.is_mapped()
            || !enabled(&widget)
        {
            return;
        }
        let start = Cell::new(None);
        *self.tick.borrow_mut() = Some(widget.add_tick_callback(move |widget, clock| {
            let start = start.get().unwrap_or_else(|| {
                start.set(Some(clock.frame_time()));
                clock.frame_time()
            });
            let phase = (clock.frame_time() - start) as f64 / 900_000. * std::f64::consts::TAU;
            widget.set_opacity(0.65 + 0.35 * phase.cos());
            glib::ControlFlow::Continue
        }));
    }
}
impl ClosingPulse {
    pub fn new(widget: &impl IsA<gtk::Widget>) -> Self {
        let widget = widget.as_ref();
        let inner = Rc::new(PulseState {
            widget: widget.downgrade(),
            pending: Cell::new(false),
            tick: RefCell::new(None),
        });
        let weak = Rc::downgrade(&inner);
        let mapped = widget.connect_map(move |_| {
            if let Some(state) = weak.upgrade() {
                state.start();
            }
        });
        let weak = Rc::downgrade(&inner);
        let unmapped = widget.connect_unmap(move |_| {
            if let Some(state) = weak.upgrade() {
                state.stop();
            }
        });
        Self {
            inner,
            signals: vec![mapped, unmapped],
        }
    }
    pub fn set_pending(&self, pending: bool) {
        self.inner.pending.set(pending);
        if let Some(widget) = self.inner.widget.upgrade() {
            if pending {
                widget.add_css_class("closing");
            } else {
                widget.remove_css_class("closing");
            }
        }
        if pending {
            self.inner.start();
        } else {
            self.inner.stop();
        }
    }
}
impl Drop for ClosingPulse {
    fn drop(&mut self) {
        self.inner.stop();
        if let Some(widget) = self.inner.widget.upgrade() {
            widget.remove_css_class("closing");
            for signal in self.signals.drain(..) {
                widget.disconnect(signal);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn pump(duration: Duration) {
        glib::MainContext::default().block_on(glib::timeout_future(duration));
    }

    #[test]
    #[ignore = "requires an isolated display; see scripts/test-motion.py"]
    fn closing_pulse_cancels_on_rebind_unmap_and_drop() {
        gtk::init().unwrap();
        let row = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let window = gtk::Window::builder()
            .title("Tjik motion lifecycle test")
            .default_width(100)
            .default_height(100)
            .child(&row)
            .build();
        let settings = row.settings();
        let animations = settings.is_gtk_enable_animations();
        settings.set_gtk_enable_animations(true);
        let pulse = ClosingPulse::new(&row);
        pulse.set_pending(true);
        assert!(pulse.inner.tick.borrow().is_none());
        window.present();
        pump(Duration::from_millis(150));
        assert!(row.is_mapped());
        assert!(pulse.inner.tick.borrow().is_some());
        assert!(
            row.opacity() < 0.99,
            "fixture must deliver animation frames"
        );
        for _ in 0..100 {
            pulse.set_pending(false);
            assert!(pulse.inner.tick.borrow().is_none());
            pulse.set_pending(true);
            assert!(pulse.inner.tick.borrow().is_some());
        }
        window.set_visible(false);
        assert!(pulse.inner.tick.borrow().is_none());
        assert_eq!(row.opacity(), 1.);
        window.present();
        pump(Duration::from_millis(100));
        assert!(pulse.inner.tick.borrow().is_some());
        drop(pulse);
        // A CSS class cannot resurrect an orphan callback after ownership ends.
        row.add_css_class("closing");
        pump(Duration::from_millis(200));
        assert_eq!(row.opacity(), 1.);
        window.close();
        settings.set_gtk_enable_animations(animations);
    }
}
