//! Stable grid with a bounded, frame-clock-driven price trace transition.
use std::{cell::RefCell, rc::Rc, time::Instant};
use tjik_gtk::gtk::{self, gdk, glib, prelude::*};

#[derive(Default)]
pub struct Trace {
    points: Vec<(f64, f64)>,
    previous: Vec<(f64, f64)>,
    started: Option<Instant>,
}
impl Trace {
    fn update(&mut self, points: Vec<(f64, f64)>) {
        // Keep cached history while refreshing; identical responses must not replay.
        if points.len() < 2 || points == self.points {
            return;
        }
        self.previous = std::mem::replace(&mut self.points, points);
        // Initial history and corrections render immediately. Only newly appended,
        // overlapping samples advance the existing time window.
        self.started = self
            .previous
            .last()
            .filter(|old| self.points.last().unwrap().0 > old.0 && self.points[0].0 <= old.0)
            .map(|_| Instant::now());
    }
    fn progress(&self) -> f64 {
        self.started
            .map(|t| (t.elapsed().as_secs_f64() / 0.5).min(1.))
            .unwrap_or(1.)
    }
}
fn range(points: &[(f64, f64)]) -> (f64, f64) {
    (
        points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
        points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
    )
}
fn line(cr: &gtk::cairo::Context, state: &Trace, width: f64, height: f64, progress: f64) {
    let current = &state.points;
    let mut start = current[0].0;
    let mut end = current.last().unwrap().0;
    let (mut lo, mut hi) = range(current);
    let mut points = Vec::new();
    if state.started.is_some() && !state.previous.is_empty() && progress < 1. {
        let mix = |old: f64, new: f64| old + (new - old) * progress;
        start = mix(state.previous[0].0, start);
        end = mix(state.previous.last().unwrap().0, end);
        let previous_range = range(&state.previous);
        lo = mix(previous_range.0, lo);
        hi = mix(previous_range.1, hi);
        points.extend(
            state
                .previous
                .iter()
                .copied()
                .take_while(|p| p.0 < current[0].0),
        );
    }
    points.extend_from_slice(current);
    let span = (end - start).max(1.);
    for (i, &(time, value)) in points.iter().enumerate() {
        let (time, value) = if time > end {
            if i == 0 {
                break;
            }
            let (x, y) = points[i - 1];
            (end, y + (value - y) * ((end - x) / (time - x).max(0.00001)))
        } else {
            (time, value)
        };
        let x = (time - start) / span * width;
        let y = 8. + height - (value - lo) / (hi - lo).max(0.00001) * height;
        if i == 0 {
            cr.move_to(x, y);
        } else {
            cr.line_to(x, y);
        }
        if time >= end {
            break;
        }
    }
    let _ = cr.stroke();
}

pub fn widget(
    state: Rc<RefCell<Trace>>,
    points: Vec<(f64, f64)>,
    accent: gdk::RGBA,
) -> gtk::DrawingArea {
    state.borrow_mut().update(points);
    let graph = gtk::DrawingArea::new();
    graph.set_content_height(190);
    graph.set_hexpand(true);
    let trace = state.clone();
    graph.set_draw_func(move |_, cr, w, h| {
        let width = (w as f64 - 62.).max(1.);
        let height = (h as f64 - 16.).max(1.);
        cr.set_source_rgba(0.65, 0.63, 0.73, 0.2);
        cr.set_line_width(1.);
        for i in 0..3 {
            let y = 8. + height * i as f64 / 2.;
            cr.move_to(0., y);
            cr.line_to(width, y);
        }
        let _ = cr.stroke();
        let state = trace.borrow();
        if state.points.len() < 2 {
            return;
        }
        let progress = state.progress();
        let _ = cr.save();
        cr.rectangle(0., 0., width, h as f64);
        cr.clip();
        cr.set_source_rgba(
            accent.red() as f64,
            accent.green() as f64,
            accent.blue() as f64,
            1.,
        );
        cr.set_line_width(2.);
        line(cr, &state, width, height, progress);
        let _ = cr.restore();
        let (lo, hi) = range(&state.points);
        cr.set_source_rgb(0.65, 0.63, 0.73);
        cr.set_font_size(10.);
        for (value, y) in [(hi, 12.), (lo, h as f64 - 3.)] {
            cr.move_to(width + 6., y);
            let _ = cr.show_text(&format!("{value:.2}"));
        }
    });
    if state.borrow().progress() < 1. {
        // Hidden workspaces may throttle the frame clock. Invalidate the final
        // frame once even then, without an always-running background timer.
        let weak = graph.downgrade();
        glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
            if let Some(graph) = weak.upgrade() {
                graph.queue_draw();
            }
        });
        graph.add_tick_callback(move |graph, _| {
            graph.queue_draw();
            if state.borrow().progress() < 1. {
                glib::ControlFlow::Continue
            } else {
                state.borrow_mut().previous.clear();
                glib::ControlFlow::Break
            }
        });
    }
    graph
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refresh_retains_history_and_identical_responses_do_not_restart() {
        let mut state = Trace::default();
        let first = vec![(0., 3.), (1., 5.)];
        state.update(first.clone());
        assert!(state.started.is_none(), "Initial history must not replay");
        let started = state.started;
        state.update(vec![]);
        state.update(first.clone());
        assert_eq!(state.started, started);
        assert_eq!(state.points, first);
        state.update(vec![(1., 5.), (2., 4.)]);
        assert_eq!(state.previous, first);
        assert!(state.started.is_some(), "Only the appended tail animates");
        state.started = Some(Instant::now() - std::time::Duration::from_millis(500));
        assert_eq!(state.progress(), 1.);
    }
}
