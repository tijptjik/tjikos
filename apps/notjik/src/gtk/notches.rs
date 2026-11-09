//! Informational edge marks, matching GroupNotches.qml's persistent split/merge slices.
use gtk::{glib, prelude::*};
use std::{cell::RefCell, rc::Rc, time::Instant};

const SLICES: usize = 60;
fn geometry(count: usize) -> [(f64, f64); SLICES] {
    let count = count.clamp(1, 5);
    let per = SLICES / count;
    std::array::from_fn(|i| {
        let band = i / per;
        let part = i % per;
        (
            (band as f64 - (count - 1) as f64 / 2.) * 10. - 3. + part as f64 * 6. / per as f64,
            6. / per as f64,
        )
    })
}
struct Motion {
    count: usize,
    from: [(f64, f64); SLICES],
    alpha: f64,
    at: Instant,
    duration: f64,
}
impl Motion {
    fn sample(&self) -> ([(f64, f64); SLICES], f64, bool) {
        let t = (self.at.elapsed().as_secs_f64() / self.duration.max(0.001)).min(1.);
        let eased = 1. - (1. - t).powi(3);
        let target = geometry(self.count);
        let alpha = if self.count > 1 { 0.75 } else { 0. };
        (
            std::array::from_fn(|i| {
                (
                    self.from[i].0 + (target[i].0 - self.from[i].0) * eased,
                    self.from[i].1 + (target[i].1 - self.from[i].1) * eased,
                )
            }),
            self.alpha + (alpha - self.alpha) * eased,
            t < 1.,
        )
    }
}
pub(super) struct Notches {
    pub widget: gtk::DrawingArea,
    motion: Rc<RefCell<Motion>>,
}
impl Notches {
    pub fn new(count: usize) -> Self {
        let widget = gtk::DrawingArea::new();
        widget.set_widget_name("group-notches");
        widget.add_css_class("group-notches");
        widget.set_halign(gtk::Align::End);
        widget.set_valign(gtk::Align::Fill);
        widget.set_can_target(false);
        widget.set_focusable(false);
        let motion = Rc::new(RefCell::new(Motion {
            count,
            from: geometry(count),
            alpha: if count > 1 { 0.75 } else { 0. },
            at: Instant::now(),
            duration: 0.220,
        }));
        let state = motion.clone();
        widget.set_draw_func(move |widget, cr, width, height| {
            let state = state.borrow();
            let (slices, alpha, moving) = state.sample();
            let color = widget.color();
            cr.set_source_rgba(
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                alpha,
            );
            let width = width as f64;
            if moving {
                cr.set_antialias(gtk::cairo::Antialias::None);
                for (y, h) in slices {
                    cr.rectangle(0., height as f64 / 2. + y, width, h);
                }
                let _ = cr.fill();
            } else {
                let count = state.count.clamp(1, 5);
                for i in 0..count {
                    let y = height as f64 / 2. + (i as f64 - (count - 1) as f64 / 2.) * 10. - 3.;
                    cr.new_sub_path();
                    for (x, y, a) in [
                        (width - 1., y + 1., -90_f64),
                        (width - 1., y + 5., 0.),
                        (1., y + 5., 90.),
                        (1., y + 1., 180.),
                    ] {
                        cr.arc(x, y, 1., a.to_radians(), (a + 90.).to_radians());
                    }
                    cr.close_path();
                }
                let _ = cr.fill();
            }
        });
        let state = motion.clone();
        let was_moving = std::cell::Cell::new(true);
        widget.add_tick_callback(move |widget, _| {
            let moving = state.borrow().sample().2;
            if moving || was_moving.get() {
                widget.queue_draw();
            }
            was_moving.set(moving);
            glib::ControlFlow::Continue
        });
        Self { widget, motion }
    }
    pub fn update(&self, count: usize) {
        self.widget
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "{count} messages"
            ))]);
        let mut state = self.motion.borrow_mut();
        if state.count == count {
            return;
        }
        let (from, alpha, _) = state.sample();
        state.from = from;
        state.alpha = alpha;
        state.count = count;
        state.at = Instant::now();
        self.widget.queue_draw();
    }
    pub fn duration(&self, milliseconds: u64) {
        self.motion.borrow_mut().duration = milliseconds as f64 / 1000.;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slices_form_centered_six_pixel_bars_with_five_bar_cap() {
        for count in 1..=7 {
            let slices = geometry(count);
            let n = count.min(5);
            let per = SLICES / n;
            for band in 0..n {
                let first = slices[band * per];
                let last = slices[(band + 1) * per - 1];
                assert!((last.0 + last.1 - first.0 - 6.).abs() < 1e-9);
                assert!((first.0 - ((band as f64 - (n - 1) as f64 / 2.) * 10. - 3.)).abs() < 1e-9);
            }
        }
    }
}
