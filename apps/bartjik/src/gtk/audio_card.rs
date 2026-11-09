//! Audio card and the original segmented meter/volume track.
use crate::{
    audio_service::{self, Action, Group, Snapshot, Update},
    panel_view::Content,
    Event,
};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};
use tjik_gtk::gtk::{self, gdk, glib, prelude::*};
fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    l.add_css_class(class);
    l
}
fn symbol(text: &str) -> gtk::Label {
    crate::panel_view::control_symbol(text)
}
pub fn meter_position(peak: f64, muted: bool) -> f64 {
    if muted || !peak.is_finite() || peak <= 0.001 {
        0.
    } else {
        ((20. * peak.log10() + 60.) / 60.).clamp(0., 1.)
    }
}
fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.).min(h / 2.);
    cr.new_sub_path();
    for (cx, cy, a) in [
        (x + w - r, y + r, -90.),
        (x + w - r, y + h - r, 0.),
        (x + r, y + h - r, 90.),
        (x + r, y + r, 180.),
    ] {
        cr.arc(
            cx,
            cy,
            r,
            a * std::f64::consts::PI / 180.,
            (a + 90.) * std::f64::consts::PI / 180.,
        );
    }
    cr.close_path();
}
fn color(cr: &gtk::cairo::Context, c: &gdk::RGBA, alpha: f64) {
    cr.set_source_rgba(
        c.red() as f64,
        c.green() as f64,
        c.blue() as f64,
        c.alpha() as f64 * alpha,
    );
}
struct Row {
    group: RefCell<Group>,
    body: gtk::Box,
    scale: gtk::Scale,
    area: gtk::DrawingArea,
    mute: gtk::Button,
    peak: Cell<f64>,
    updating: Cell<bool>,
    pressed: Cell<bool>,
}
pub struct Card {
    pub body: gtk::Box,
    master: gtk::Box,
    clients: gtk::Box,
    route: gtk::Button,
    message: gtk::Label,
    rows: RefCell<Vec<Rc<Row>>>,
    session: RefCell<Option<audio_service::Session>>,
    timers: RefCell<BTreeMap<String, glib::SourceId>>,
    pending: Cell<usize>,
    epoch: Cell<u64>,
    demo: bool,
    colors: Rc<RefCell<[gdk::RGBA; 3]>>,
    send: async_channel::Sender<Event>,
}
impl Drop for Card {
    fn drop(&mut self) {
        for (_, timer) in self.timers.get_mut().split_off("") {
            timer.remove();
        }
    }
}
impl Card {
    pub fn new(demo: bool, send: async_channel::Sender<Event>) -> Rc<Self> {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let caption = label("Master", "network-muted");
        caption.set_margin_top(4);
        body.append(&caption);
        let master = gtk::Box::new(gtk::Orientation::Vertical, 4);
        master.add_css_class("network-connection");
        body.append(&master);
        let caption = label("Clients", "network-muted");
        caption.set_margin_top(4);
        body.append(&caption);
        let clients = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(260)
            .child(&clients)
            .build();
        body.append(&scroll);
        let message = label("Loading audio…", "network-muted");
        message.set_wrap(true);
        body.append(&message);
        let route = gtk::Button::new();
        route.add_css_class("audio-route");
        route.set_margin_top(4);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        line.set_halign(gtk::Align::Center);
        line.append(&symbol("headphones"));
        line.append(&label("HEADPHONES", "network-title"));
        route.set_child(Some(&line));
        route.set_sensitive(false);
        route.update_property(&[gtk::accessible::Property::Label(
            "Switch between headphones and screen",
        )]);
        body.append(&route);
        let card = Rc::new(Self {
            body,
            master,
            clients,
            route,
            message,
            rows: RefCell::new(vec![]),
            session: RefCell::new(None),
            timers: RefCell::new(BTreeMap::new()),
            pending: Cell::new(0),
            epoch: Cell::new(0),
            demo,
            colors: Rc::new(RefCell::new([
                gdk::RGBA::parse("#ff53a6").unwrap(),
                gdk::RGBA::parse("#1f1d2e").unwrap(),
                gdk::RGBA::WHITE,
            ])),
            send,
        });
        let weak = Rc::downgrade(&card);
        card.route.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.run(Action::Route);
            }
        });
        let weak = Rc::downgrade(&card);
        card.body.connect_map(move |_| {
            if let Some(c) = weak.upgrade() {
                c.start();
            }
        });
        let weak = Rc::downgrade(&card);
        card.body.connect_unmap(move |_| {
            if let Some(c) = weak.upgrade() {
                c.stop();
            }
        });
        card
    }
    pub fn content(&self, style: &Value) -> Content {
        for (i, key) in ["accent", "background", "text"].iter().enumerate() {
            if let Some(c) = style[key].as_str().and_then(|s| gdk::RGBA::parse(s).ok()) {
                self.colors.borrow_mut()[i] = c;
            }
        }
        Content {
            title: "Audio".into(),
            body: self.body.clone().upcast(),
            footer: None,
            preferred_width: 420.,
            needs_keyboard: true,
        }
    }
    fn resized(&self) {
        let _ = self.send.try_send(Event::PanelCommand("resize".into()));
    }
    fn stop(&self) {
        self.epoch.set(self.epoch.get() + 1);
        self.session.borrow_mut().take();
        for (_, timer) in std::mem::take(&mut *self.timers.borrow_mut()) {
            timer.remove();
        }
        self.pending.set(0);
        for r in self.rows.borrow().iter() {
            r.peak.set(0.);
            r.area.queue_draw();
        }
    }
    fn start(self: &Rc<Self>) {
        if self.session.borrow().is_some() {
            return;
        }
        let (session, updates) = audio_service::start(self.demo);
        *self.session.borrow_mut() = Some(session);
        let epoch = self.epoch.get();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(update) = updates.recv().await {
                let Some(c) = weak.upgrade() else {
                    break;
                };
                if c.epoch.get() != epoch {
                    break;
                }
                match update {
                    Update::State(result, complete) => {
                        if complete {
                            c.pending.set(c.pending.get().saturating_sub(1));
                        }
                        match result {
                            Ok(state) => {
                                c.message.set_visible(false);
                                c.apply(state);
                            }
                            Err(error) => {
                                for row in c.rows.borrow().iter() {
                                    row.scale.set_sensitive(false);
                                    row.mute.set_sensitive(false);
                                    row.peak.set(0.);
                                    row.area.queue_draw();
                                }
                                c.route.set_sensitive(false);
                                c.message.set_text(&error);
                                c.message.set_visible(true);
                            }
                        }
                        c.resized();
                    }
                    Update::Peaks(values) => {
                        for r in c.rows.borrow().iter() {
                            r.peak
                                .set(values[&r.group.borrow().key].as_f64().unwrap_or(0.));
                            r.area.queue_draw();
                        }
                    }
                }
            }
        });
    }
    fn run(&self, action: Action) {
        let result = self
            .session
            .borrow()
            .as_ref()
            .ok_or("Audio panel is not active".to_owned())
            .and_then(|s| s.send(action));
        match result {
            Ok(()) => {
                self.pending.set(self.pending.get() + 1);
                self.route.set_sensitive(false);
                for r in self.rows.borrow().iter() {
                    r.mute.set_sensitive(false);
                }
            }
            Err(e) => {
                self.message.set_text(&e);
                self.message.set_visible(true);
                self.resized();
            }
        }
    }
    fn schedule(self: &Rc<Self>, row: &Rc<Row>) {
        let key = row.group.borrow().key.clone();
        if let Some(timer) = self.timers.borrow_mut().remove(&key) {
            timer.remove();
        }
        let weak = Rc::downgrade(self);
        let r = Rc::downgrade(row);
        let k = key.clone();
        let timer =
            glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
                if let (Some(c), Some(r)) = (weak.upgrade(), r.upgrade()) {
                    c.timers.borrow_mut().remove(&k);
                    c.run(Action::Volume(
                        r.group.borrow().targets.clone(),
                        r.scale.value(),
                    ));
                }
            });
        self.timers.borrow_mut().insert(key, timer);
    }
    fn apply(self: &Rc<Self>, state: Snapshot) {
        let changed = {
            let rows = self.rows.borrow();
            rows.len() != state.groups.len()
                || rows.iter().zip(&state.groups).any(|(r, g)| {
                    let old = r.group.borrow();
                    old.targets != g.targets || old.key != g.key || old.detail != g.detail
                })
        };
        if changed {
            for (_, timer) in std::mem::take(&mut *self.timers.borrow_mut()) {
                timer.remove();
            }
            for box_ in [&self.master, &self.clients] {
                while let Some(child) = box_.first_child() {
                    box_.remove(&child);
                }
            }
            self.rows.borrow_mut().clear();
            for group in &state.groups {
                let r = self.make_row(group.clone());
                if group.master {
                    if self.master.first_child().is_some() {
                        self.master
                            .append(&gtk::Separator::new(gtk::Orientation::Horizontal));
                    }
                    self.master.append(&r.body);
                } else {
                    self.clients.append(&r.body);
                }
                self.rows.borrow_mut().push(r);
            }
            if self.master.first_child().is_none() {
                self.master
                    .append(&label("No output device available", "network-muted"));
            }
            if self.clients.first_child().is_none() {
                self.clients
                    .append(&label("No playback clients", "network-muted"));
            }
        }
        for (r, g) in self.rows.borrow().iter().zip(state.groups) {
            if self.pending.get() == 0
                && !self.timers.borrow().contains_key(&g.key)
                && !r.pressed.get()
            {
                r.updating.set(true);
                r.scale.set_value(g.volume.clamp(0., 1.));
                r.updating.set(false);
            }
            r.scale.set_sensitive(true);
            r.mute.set_sensitive(self.pending.get() == 0);
            if g.muted {
                r.mute.add_css_class("network-accent");
            } else {
                r.mute.remove_css_class("network-accent");
            }
            r.body.set_opacity(if g.muted { 0.5 } else { 1. });
            r.mute.set_child(Some(&symbol(if g.microphone {
                if g.muted {
                    "mic_off"
                } else {
                    "mic"
                }
            } else if g.muted {
                "volume_off"
            } else {
                "volume_up"
            })));
            r.mute
                .update_property(&[gtk::accessible::Property::Label(&format!(
                    "{} {}",
                    if g.muted { "Unmute" } else { "Mute" },
                    g.title
                ))]);
            *r.group.borrow_mut() = g;
            r.area.queue_draw();
        }
        self.route
            .set_sensitive(state.route && self.pending.get() == 0);
        if state.headphones {
            self.route.add_css_class("audio-route-active");
        } else {
            self.route.remove_css_class("audio-route-active");
        }
    }
    fn make_row(self: &Rc<Self>, group: Group) -> Rc<Row> {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 4);
        if !group.master {
            body.add_css_class("network-connection");
        }
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title = label(
            if group.master {
                &group.detail
            } else {
                &group.title
            },
            "network-label",
        );
        title.set_hexpand(true);
        top.append(&title);
        let mute = gtk::Button::new();
        mute.add_css_class("panel-control");
        mute.set_valign(gtk::Align::Center);
        top.append(&mute);
        body.append(&top);
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0., 1., 0.01);
        scale.set_draw_value(false);
        scale.add_css_class("audio-scale");
        scale.set_size_request(-1, 36);
        scale.set_value(group.volume.clamp(0., 1.));
        scale.update_property(&[gtk::accessible::Property::Label(&format!(
            "{} volume",
            group.title
        ))]);
        let area = gtk::DrawingArea::new();
        area.set_content_height(36);
        area.set_hexpand(true);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&area));
        overlay.add_overlay(&scale);
        scale.set_hexpand(true);
        scale.set_halign(gtk::Align::Fill);
        body.append(&overlay);
        let r = Rc::new(Row {
            group: RefCell::new(group),
            body,
            scale,
            area,
            mute,
            peak: Cell::new(if self.demo { 0.08 } else { 0. }),
            updating: Cell::new(false),
            pressed: Cell::new(false),
        });
        let weak = Rc::downgrade(&r);
        let colors = self.colors.clone();
        r.area.set_draw_func(move |_, cr, w, h| {
            let Some(r) = weak.upgrade() else {
                return;
            };
            let colors = colors.borrow();
            let width = w as f64;
            let y = h as f64 / 2.;
            let volume = r.scale.value();
            let meter = meter_position(r.peak.get(), r.group.borrow().muted);
            let bar = ((width - 117.) / 40.).max(0.);
            for i in 0..40 {
                let position = (i as f64 + 0.5) / 40.;
                let height = if i % 5 == 4 { 16. } else { 12. };
                if position <= meter {
                    color(cr, &colors[0], 1.);
                } else if position <= volume {
                    color(cr, &colors[0], 0.25);
                } else {
                    color(cr, &colors[1], 1.);
                }
                rounded(cr, i as f64 * (bar + 3.), y - height / 2., bar, height, 2.);
                let _ = cr.fill();
            }
            let x = volume * (width - 16.).max(0.);
            rounded(cr, x + 1., y - 12., 14., 24., 4.);
            color(
                cr,
                if r.pressed.get() {
                    &colors[2]
                } else {
                    &colors[0]
                },
                1.,
            );
            let _ = cr.fill_preserve();
            color(cr, &colors[1], 1.);
            cr.set_line_width(2.);
            let _ = cr.stroke();
            rounded(cr, x + 7., y - 5., 2., 10., 1.);
            let _ = cr.fill();
        });
        let weak = Rc::downgrade(&r);
        let card = Rc::downgrade(self);
        r.scale.connect_value_changed(move |_| {
            if let (Some(r), Some(c)) = (weak.upgrade(), card.upgrade()) {
                r.area.queue_draw();
                if !r.updating.get() {
                    c.schedule(&r);
                }
            }
        });
        let gesture = gtk::GestureClick::new();
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&r);
        gesture.connect_pressed(move |_, _, _, _| {
            if let Some(r) = weak.upgrade() {
                r.pressed.set(true);
                r.area.queue_draw();
            }
        });
        let weak = Rc::downgrade(&r);
        gesture.connect_released(move |_, _, _, _| {
            if let Some(r) = weak.upgrade() {
                r.pressed.set(false);
                r.area.queue_draw();
            }
        });
        let weak = Rc::downgrade(&r);
        gesture.connect_stopped(move |_| {
            if let Some(r) = weak.upgrade() {
                r.pressed.set(false);
                r.area.queue_draw();
            }
        });
        r.scale.add_controller(gesture);
        let weak = Rc::downgrade(&r);
        let card = Rc::downgrade(self);
        r.mute.connect_clicked(move |_| {
            if let (Some(r), Some(c)) = (weak.upgrade(), card.upgrade()) {
                let g = r.group.borrow();
                c.run(Action::Mute(g.targets.clone(), !g.muted));
            }
        });
        r
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn meter_uses_db_and_does_not_move_volume() {
        assert_eq!(meter_position(0., false), 0.);
        assert_eq!(meter_position(1., true), 0.);
        assert!((meter_position(0.1, false) - 2. / 3.).abs() < 1e-9);
        assert_eq!(meter_position(1., false), 1.);
    }
}

#[cfg(test)]
pub fn exercise_fixture(card: &Rc<Card>) {
    fn wait(mut ready: impl FnMut() -> bool) {
        let context = glib::MainContext::default();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !ready() && std::time::Instant::now() < until {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(ready());
    }
    assert_eq!(card.rows.borrow().len(), 3);
    assert!(card.body.measure(gtk::Orientation::Horizontal, -1).0 <= 388);
    let output = card.rows.borrow()[0].clone();
    let level = output.scale.value();
    output.peak.set(1.);
    output.area.queue_draw();
    assert_eq!(output.scale.value(), level, "meter moved the volume handle");
    for i in 0..30 {
        output.scale.set_value(i as f64 / 100.);
    }
    assert_eq!(
        card.timers.borrow().len(),
        1,
        "slider updates were not coalesced"
    );
    wait(|| {
        card.timers.borrow().is_empty()
            && card.pending.get() == 0
            && (output.group.borrow().volume - 0.29).abs() < 0.001
    });
    output.mute.emit_clicked();
    wait(|| card.pending.get() == 0 && output.group.borrow().muted);
    assert!((output.body.opacity() - 0.5).abs() <= 1. / 255.);
    card.route.emit_clicked();
    wait(|| card.pending.get() == 0 && !card.route.has_css_class("audio-route-active"));
    card.stop();
    assert!(card.session.borrow().is_none());
    assert!(card.timers.borrow().is_empty());
}
