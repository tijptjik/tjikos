//! One fixed marker surface: dot, selection and completion share a center.
use std::{cell::Cell, rc::Rc};
use tjik_gtk::gtk::{self, glib, prelude::*};
#[derive(Clone)]
pub(crate) struct Marker {
    pub button: gtk::Button,
    area: gtk::DrawingArea,
    date: gtk::Label,
    selected: Rc<Cell<bool>>,
    hover: Rc<Cell<bool>>,
    progress: Rc<Cell<f64>>,
    running: Rc<Cell<bool>>,
    completed: Rc<Cell<bool>>,
    check: Rc<Cell<f64>>,
}
impl Marker {
    pub fn new(date: &str, done: bool, height: i32, action: impl Fn() + 'static) -> Self {
        let button = gtk::Button::new();
        button.add_css_class("completion-marker");
        button.set_focusable(false);
        button.set_valign(gtk::Align::Start);
        button.set_size_request(28, height);
        button.set_tooltip_text(Some(if done { "Reopen" } else { "Complete" }));
        button.connect_clicked(move |_| action());
        let area = gtk::DrawingArea::builder()
            .content_width(28)
            .content_height(height)
            .build();
        let label = gtk::Label::new(Some(if date == "·" { "" } else { date }));
        label.add_css_class("task-date");
        label.set_can_target(false);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&area));
        overlay.add_overlay(&label);
        button.set_child(Some(&overlay));
        let this = Self {
            button,
            area,
            date: label,
            selected: Rc::new(Cell::new(false)),
            hover: Rc::new(Cell::new(done)),
            progress: Rc::new(Cell::new(if done { 1. } else { 0. })),
            running: Rc::new(Cell::new(false)),
            completed: Rc::new(Cell::new(done)),
            check: Rc::new(Cell::new(if done { 1. } else { 0. })),
        };
        let selected = this.selected.clone();
        let progress = this.progress.clone();
        let dot = date == "·";
        let completed = this.completed.clone();
        let check = this.check.clone();
        this.area.set_draw_func(move |_, c, width, height| {
            let t = progress.get();
            let x = width as f64 / 2.;
            let y = height as f64 / 2.;
            let accent = (168. / 255., 199. / 255., 160. / 255.);
            c.set_line_cap(gtk::cairo::LineCap::Round);
            c.set_line_join(gtk::cairo::LineJoin::Round);
            if t < 1. {
                if selected.get() {
                    c.set_source_rgba(accent.0, accent.1, accent.2, 1. - t);
                    c.set_line_width(1.8);
                    c.move_to(x - 2.5, y - 4.5);
                    c.line_to(x + 2.5, y);
                    c.line_to(x - 2.5, y + 4.5);
                    let _ = c.stroke();
                } else if dot {
                    c.set_source_rgba(170. / 255., 165. / 255., 186. / 255., 1. - t);
                    c.arc(x, y, 2., 0., std::f64::consts::TAU);
                    let _ = c.fill();
                }
            }
            if t > 0. {
                // Symmetric expansion around the existing dot. No widget swap,
                // native checkbox padding, or drifting font bearings.
                let half = 2. + 5. * t;
                let radius = half.min(3.);
                c.new_sub_path();
                for (cx, cy, start) in [
                    (x + half - radius, y - half + radius, -90.0_f64),
                    (x + half - radius, y + half - radius, 0.),
                    (x - half + radius, y + half - radius, 90.),
                    (x - half + radius, y - half + radius, 180.),
                ] {
                    c.arc(
                        cx,
                        cy,
                        radius,
                        start.to_radians(),
                        (start + 90.).to_radians(),
                    );
                }
                c.close_path();
                c.set_source_rgba(accent.0, accent.1, accent.2, t);
                if completed.get() {
                    let _ = c.fill_preserve();
                }
                c.set_line_width(1.3);
                let _ = c.stroke();
                if completed.get() {
                    c.set_source_rgba(38. / 255., 35. / 255., 58. / 255., t);
                    c.set_line_width(1.5);
                    c.move_to(x - 3.5, y);
                    let t = check.get();
                    let first = (t / 0.32).min(1.);
                    c.line_to(x - 3.5 + 2.5 * first, y + 2.5 * first);
                    if t > 0.32 {
                        let second = (t - 0.32) / 0.68;
                        c.line_to(x - 1. + 5. * second, y + 2.5 - 5.5 * second);
                    }
                    let _ = c.stroke();
                }
            }
        });
        this.paint();
        this
    }
    pub fn complete(&self) {
        self.completed.set(true);
        self.hover.set(true);
        self.progress.set(1.);
        self.check.set(0.);
        self.paint();
        let this = self.clone();
        let start = Cell::new(None);
        self.area.add_tick_callback(move |_, clock| {
            let now = clock.frame_time();
            let begin = start.get().unwrap_or(now);
            start.set(Some(begin));
            let t = ((now - begin) as f64 / 220_000.).clamp(0., 1.);
            this.check.set(t);
            this.paint();
            if t >= 1. {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
    pub fn set_selected(&self, selected: bool) {
        self.selected.set(selected);
        self.paint();
    }
    fn paint(&self) {
        self.date.set_opacity(if self.selected.get() {
            0.
        } else {
            1. - self.progress.get()
        });
        self.area.queue_draw();
    }
    pub fn follow_focus(&self, owner: &impl IsA<gtk::Widget>) {
        let this = self.clone();
        owner.connect_notify_local(Some("has-focus"), move |w, _| {
            this.set_selected(w.has_focus())
        });
    }
    pub fn hover_on(&self, owner: &impl IsA<gtk::Widget>) {
        let motion = gtk::EventControllerMotion::new();
        let weak = self.button.downgrade();
        let state = self.clone();
        // Store only the animation state in the controller; the button is owned
        // by its row, not by its own event handler.
        let parts = (
            state.area.downgrade(),
            state.date.downgrade(),
            state.selected,
            state.hover,
            state.progress,
            state.running,
        );
        let completed = self.completed.clone();
        let check = self.check.clone();
        let enter_completed = completed.clone();
        let enter_check = check.clone();
        let enter_parts = parts.clone();
        let enter_button = weak.clone();
        motion.connect_enter(move |_, _, _| {
            if let (Some(button), Some(area), Some(date)) = (
                enter_button.upgrade(),
                enter_parts.0.upgrade(),
                enter_parts.1.upgrade(),
            ) {
                Self {
                    button,
                    area,
                    date,
                    selected: enter_parts.2.clone(),
                    hover: enter_parts.3.clone(),
                    progress: enter_parts.4.clone(),
                    running: enter_parts.5.clone(),
                    completed: enter_completed.clone(),
                    check: enter_check.clone(),
                }
                .animate(true);
            }
        });
        motion.connect_leave(move |_| {
            if let (Some(button), Some(area), Some(date)) =
                (weak.upgrade(), parts.0.upgrade(), parts.1.upgrade())
            {
                Self {
                    button,
                    area,
                    date,
                    selected: parts.2.clone(),
                    hover: parts.3.clone(),
                    progress: parts.4.clone(),
                    running: parts.5.clone(),
                    completed: completed.clone(),
                    check: check.clone(),
                }
                .animate(false);
            }
        });
        owner.add_controller(motion);
    }
    fn animate(&self, hover: bool) {
        self.hover
            .set(self.completed.get() || (hover && self.button.is_sensitive()));
        if self.running.replace(true) {
            return;
        }
        let this = self.clone();
        let previous = Cell::new(None);
        self.area.add_tick_callback(move |_, clock| {
            let now = clock.frame_time();
            let elapsed = previous
                .get()
                .map(|p| (now - p) as f64 / 160_000.)
                .unwrap_or(0.);
            previous.set(Some(now));
            let target = if this.hover.get() { 1. } else { 0. };
            let next = if target > this.progress.get() {
                (this.progress.get() + elapsed).min(target)
            } else {
                (this.progress.get() - elapsed).max(target)
            };
            this.progress.set(next);
            this.paint();
            if next == target {
                this.running.set(false);
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
}
