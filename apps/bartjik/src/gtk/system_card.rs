//! GTK policy editors. Persistent policy, authorization and recovery stay in core.
use crate::{
    monitor_geometry as geometry,
    panel_view::Content,
    util::{arr, num, s, st},
    Event,
};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    time::Duration,
};
use tjik_gtk::gtk::{self, glib, prelude::*};
fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    l.add_css_class(class);
    l
}
fn line() -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, 12)
}
fn caption(text: &str) -> gtk::Label {
    let l = label(text, "policy-muted");
    l.set_wrap(true);
    l.set_ellipsize(gtk::pango::EllipsizeMode::None);
    l
}
fn button(text: &str, name: &str) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.set_widget_name(name);
    b.add_css_class("policy-button");
    b
}
fn entry(text: &str, name: &str) -> gtk::Entry {
    let e = gtk::Entry::new();
    e.set_text(text);
    e.set_widget_name(name);
    e.set_hexpand(true);
    e.set_width_chars(5);
    e.add_css_class("policy-field");
    e.update_property(&[gtk::accessible::Property::Label(name)]);
    e
}
fn duration(n: i64) -> String {
    if n == 0 {
        "Never".into()
    } else if n < 60 {
        format!("{n} sec")
    } else if n < 3600 {
        format!("{} min", n / 60)
    } else {
        format!("{} hr", n as f64 / 3600.)
    }
}
fn choice(
    values: Vec<(Value, String)>,
    selected: &Value,
    name: &str,
    changed: impl Fn(Value) + 'static,
) -> gtk::DropDown {
    let names: Vec<_> = values.iter().map(|(_, s)| s.as_str()).collect();
    let c = gtk::DropDown::from_strings(&names);
    c.set_widget_name(name);
    c.add_css_class("policy-choice");
    c.set_hexpand(true);
    c.set_selected(
        values
            .iter()
            .position(|(v, _)| {
                v == selected
                    || (v.is_number() && selected.is_number() && v.as_f64() == selected.as_f64())
            })
            .map(|n| n as u32)
            .unwrap_or(gtk::INVALID_LIST_POSITION),
    );
    c.update_property(&[gtk::accessible::Property::Label(name)]);
    c.connect_selected_notify(move |c| {
        if let Some((v, _)) = values.get(c.selected() as usize) {
            changed(v.clone());
        }
    });
    c
}
fn pair(body: &gtk::Box, title: &str, control: &impl IsA<gtk::Widget>) {
    let row = line();
    control.set_hexpand(false);
    control.set_size_request(200, -1);
    let l = label(title, "policy-label");
    l.set_hexpand(true);
    row.append(&l);
    row.append(control);
    body.append(&row);
}
pub struct Card {
    pub provider: &'static str,
    body: gtk::Box,
    info: RefCell<Value>,
    context: RefCell<Value>,
    style: RefCell<Value>,
    draft: RefCell<Value>,
    basis: RefCell<Value>,
    revision: RefCell<Value>,
    selected: RefCell<String>,
    dirty: Cell<bool>,
    pending: Cell<Option<u64>>,
    pending_context: RefCell<Value>,
    sequence: Cell<u64>,
    queue: RefCell<Vec<(String, String)>>,
    timers: RefCell<BTreeMap<String, glib::SourceId>>,
    clock: RefCell<Option<glib::SourceId>>,
    countdown: gtk::Label,
    feedback: RefCell<String>,
    confirm_power: Cell<bool>,
    change: RefCell<Value>,
    send: async_channel::Sender<Event>,
}
impl Card {
    pub fn new(provider: &'static str, send: async_channel::Sender<Event>) -> Rc<Self> {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 16);
        body.add_css_class("policy-card");
        body.set_focusable(true);
        let c = Rc::new(Self {
            provider,
            body,
            info: RefCell::new(Value::Null),
            context: RefCell::new(Value::Null),
            style: RefCell::new(Value::Null),
            draft: RefCell::new(Value::Null),
            basis: RefCell::new(Value::Null),
            revision: RefCell::new(Value::Null),
            selected: RefCell::new(String::new()),
            dirty: Cell::new(false),
            pending: Cell::new(None),
            pending_context: RefCell::new(Value::Null),
            sequence: Cell::new(0),
            queue: RefCell::new(vec![]),
            timers: RefCell::new(BTreeMap::new()),
            clock: RefCell::new(None),
            countdown: caption(""),
            feedback: RefCell::new(String::new()),
            confirm_power: Cell::new(false),
            change: RefCell::new(Value::Null),
            send,
        });
        let weak = Rc::downgrade(&c);
        c.body.connect_map(move |_| {
            if let Some(c) = weak.upgrade() {
                c.render();
                let weak = Rc::downgrade(&c);
                *c.clock.borrow_mut() =
                    Some(glib::timeout_add_local(Duration::from_secs(1), move || {
                        let Some(c) = weak.upgrade() else {
                            return glib::ControlFlow::Break;
                        };
                        c.tick();
                        glib::ControlFlow::Continue
                    }));
                c.tick();
            }
        });
        let weak = Rc::downgrade(&c);
        c.body.connect_unmap(move |_| {
            if let Some(c) = weak.upgrade() {
                if let Some(t) = c.clock.borrow_mut().take() {
                    t.remove();
                }
                for (_, t) in std::mem::take(&mut *c.timers.borrow_mut()) {
                    t.remove();
                }
                c.queue.borrow_mut().clear();
                c.confirm_power.set(false);
                c.reset_draft();
            }
        });
        c
    }
    pub fn content(&self) -> Content {
        Content {
            title: match self.provider {
                "caffeine" => "Idle & Sleep",
                "nightlight" => "Night light",
                _ => "Monitor Setup",
            }
            .into(),
            body: self.body.clone().upcast(),
            footer: None,
            preferred_width: if self.provider == "monitor-layout" {
                520.
            } else {
                420.
            },
            needs_keyboard: true,
        }
    }
    pub fn update(self: &Rc<Self>, info: Value, context: Value, style: Value) {
        let changed = *self.info.borrow() != info
            || *self.context.borrow() != context
            || *self.style.borrow() != style;
        *self.context.borrow_mut() = context;
        *self.style.borrow_mut() = style;
        if !changed {
            return;
        }
        *self.info.borrow_mut() = info;
        if !self.dirty.get() {
            self.reset_draft();
        } else if self.provider == "monitor-layout" {
            let live = self.info.borrow();
            for m in self.draft.borrow_mut().as_array_mut().into_iter().flatten() {
                let found = arr(&live["monitors"])
                    .iter()
                    .find(|n| n["name"] == m["name"]);
                m["connected"] = json!(found.is_some_and(|v| v["connected"] == true));
            }
        }
        let editing = self
            .body
            .root()
            .and_downcast::<gtk::Window>()
            .and_then(|w| gtk::prelude::GtkWindowExt::focus(&w))
            .is_some_and(|w| w.is::<gtk::Editable>() || w.is::<gtk::TextView>());
        if (!editing && self.timers.borrow().is_empty()) || !self.body.is_mapped() {
            self.render();
        }
    }
    fn reset_draft(&self) {
        let i = self.info.borrow();
        *self.draft.borrow_mut() = i[if self.provider == "monitor-layout" {
            "monitors"
        } else {
            "delays"
        }]
        .clone();
        *self.basis.borrow_mut() = self.draft.borrow().clone();
        *self.revision.borrow_mut() = i["revision"].clone();
        self.dirty.set(false);
        if self.provider == "monitor-layout"
            && !arr(&self.draft.borrow())
                .iter()
                .any(|m| s(&m["name"]) == *self.selected.borrow())
        {
            *self.selected.borrow_mut() = arr(&self.draft.borrow())
                .first()
                .map(|m| s(&m["name"]).to_owned())
                .unwrap_or_default();
        }
    }
    pub fn acknowledge(self: &Rc<Self>, v: &Value) {
        if self.pending.get().is_some()
            && v["requestId"].as_u64() == self.pending.get()
            && v["module"] == self.pending_context.borrow()["module"]
            && v["barKey"] == self.pending_context.borrow()["barKey"]
            && v["busy"] != true
        {
            self.finish(s(&v["message"]), v["error"] == true);
        }
    }
    pub fn finish(self: &Rc<Self>, message: &str, error: bool) {
        self.pending.set(None);
        *self.feedback.borrow_mut() = if error { message.into() } else { String::new() };
        if !error && self.provider == "caffeine" {
            self.dirty.set(false);
        }
        self.render();
        let next = {
            let mut q = self.queue.borrow_mut();
            if q.is_empty() {
                None
            } else {
                Some(q.remove(0))
            }
        };
        if let Some((a, v)) = next {
            self.run(&a, v);
        }
    }
    pub fn monitor_change(self: &Rc<Self>, change: Value) {
        if self.provider != "monitor-layout" || *self.change.borrow() == change {
            return;
        }
        let ended = !self.change.borrow().is_null() && change.is_null();
        *self.change.borrow_mut() = change;
        if ended {
            self.reset_draft();
        }
        self.render();
    }
    fn run(self: &Rc<Self>, action: &str, selection: String) {
        if self.pending.get().is_some() {
            let mut q = self.queue.borrow_mut();
            q.retain(|(a, _)| a != action);
            if q.len() < 8 {
                q.push((action.into(), selection));
            }
            return;
        }
        let id = self.sequence.get() + 1;
        self.sequence.set(id);
        self.pending.set(Some(id));
        let mut context = self.context.borrow().clone();
        context["action"] = json!(action);
        context["selection"] = json!(selection);
        context["requestId"] = json!(id);
        *self.pending_context.borrow_mut() = context.clone();
        self.feedback.borrow_mut().clear();
        self.body.grab_focus();
        if self
            .send
            .try_send(Event::SystemAction(
                self.provider.into(),
                json!({"op":"panelAction","context":context}),
            ))
            .is_err()
        {
            self.finish("Backend queue unavailable", true);
            return;
        }
        self.render();
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_secs(40), move || {
            if let Some(c) = weak.upgrade() {
                if c.pending.get() == Some(id) {
                    c.finish("Request timed out; check current service state", true);
                }
            }
        });
    }
    fn debounce(self: &Rc<Self>, key: &str, value: String) {
        if let Some(t) = self.timers.borrow_mut().remove(key) {
            t.remove();
        }
        let name = key.to_owned();
        let weak = Rc::downgrade(self);
        let t = glib::timeout_add_local_once(Duration::from_millis(350), move || {
            if let Some(c) = weak.upgrade() {
                c.timers.borrow_mut().remove(&name);
                c.run(&name, value);
            }
        });
        self.timers.borrow_mut().insert(key.into(), t);
    }
    fn act(self: &Rc<Self>, text: &str, name: &str, value: String) -> gtk::Button {
        let b = button(text, name);
        b.set_sensitive(self.pending.get().is_none());
        let c = Rc::downgrade(self);
        let action = name.to_owned();
        b.connect_clicked(move |_| {
            if let Some(c) = c.upgrade() {
                c.run(&action, value.clone());
            }
        });
        b
    }
    fn toggle(
        self: &Rc<Self>,
        title: &str,
        active: bool,
        name: &str,
        changed: impl Fn(bool) + 'static,
    ) -> gtk::Box {
        let row = line();
        let l = label(title, "policy-label");
        l.set_hexpand(true);
        row.append(&l);
        let sw = gtk::Switch::new();
        sw.set_widget_name(name);
        sw.add_css_class("policy-toggle");
        sw.set_valign(gtk::Align::Center);
        sw.set_active(active);
        sw.set_sensitive(self.pending.get().is_none());
        sw.update_property(&[gtk::accessible::Property::Label(title)]);
        sw.connect_state_set(move |_, state| {
            changed(state);
            glib::Propagation::Stop
        });
        row.append(&sw);
        row
    }
    fn divider(&self) {
        self.body
            .append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    }
    fn tick(&self) {
        let now = chrono::Local::now().timestamp_millis() as f64 / 1000.;
        let change = self.change.borrow();
        let info = self.info.borrow();
        let text = if !change.is_null() {
            format!(
                "Previous layout returns in {} seconds.",
                (num(&change["deadline"]) - now).ceil().max(0.) as u64
            )
        } else if self.provider == "caffeine"
            && info["inhibited"] == true
            && num(&info["deadline"]) > 0.
        {
            format!(
                "Resumes automatically in {} min",
                ((num(&info["deadline"]) - now) / 60.).ceil().max(0.) as u64
            )
        } else {
            String::new()
        };
        self.countdown.set_text(&text);
        self.countdown.set_visible(!text.is_empty());
    }
    fn render(self: &Rc<Self>) {
        while let Some(w) = self.body.first_child() {
            self.body.remove(&w);
        }
        match self.provider {
            "caffeine" => self.idle(),
            "nightlight" => self.nightlight(),
            _ => self.monitors(),
        };
        self.tick();
        if self.body.is_mapped() {
            let _ = self.send.try_send(Event::PanelCommand("resize".into()));
        }
    }
    fn idle(self: &Rc<Self>) {
        let i = self.info.borrow().clone();
        let weak = Rc::downgrade(self);
        self.body.append(&self.toggle(
            "Keep awake",
            i["inhibited"] == true,
            "idle-enabled",
            move |v| {
                if let Some(c) = weak.upgrade() {
                    c.run("enabled", if v { "on" } else { "off" }.into());
                }
            },
        ));
        self.body.append(&self.countdown);
        let weak = Rc::downgrade(self);
        let durations = choice(
            vec![
                (json!(1800), "30 min".into()),
                (json!(3600), "1 hr".into()),
                (json!(7200), "2 hr".into()),
                (json!(0), "Until off".into()),
            ],
            &i["duration"],
            "idle-duration",
            move |v| {
                if let Some(c) = weak.upgrade() {
                    c.run("duration", st(&v));
                }
            },
        );
        durations.set_sensitive(self.pending.get().is_none());
        pair(&self.body, "Keep awake for", &durations);
        self.divider();
        self.body.append(&label("When inactive", "policy-label"));
        if i["gamePolicy"] == true {
            self.body
                .append(&caption("Using your game-mode idle policy"));
        }
        for (key, title) in [
            ("dim", "Dim screens"),
            ("lock", "Lock session"),
            ("screenOff", "Turn screens off"),
            ("suspend", "Sleep"),
        ] {
            let value = self.draft.borrow()[key].clone();
            let mut seconds = vec![
                0,
                15,
                30,
                60,
                120,
                180,
                300,
                600,
                900,
                1800,
                3600,
                7200,
                14400,
                28800,
                86400,
                value.as_i64().unwrap_or(0),
            ];
            seconds.sort();
            seconds.dedup();
            let weak = Rc::downgrade(self);
            let c = choice(
                seconds
                    .into_iter()
                    .map(|n| (json!(n), duration(n)))
                    .collect(),
                &value,
                &format!("idle-delay-{key}"),
                move |v| {
                    if let Some(c) = weak.upgrade() {
                        c.draft.borrow_mut()[key] = v;
                        c.dirty.set(*c.draft.borrow() != c.info.borrow()["delays"]);
                        c.render();
                    }
                },
            );
            c.set_sensitive(i["editable"] == true && self.pending.get().is_none());
            pair(&self.body, title, &c);
        }
        let buttons = line();
        buttons.set_halign(gtk::Align::End);
        let reset = button("Reset", "idle-reset");
        reset.set_sensitive(self.dirty.get() && self.pending.get().is_none());
        let weak = Rc::downgrade(self);
        reset.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.reset_draft();
                c.render();
            }
        });
        buttons.append(&reset);
        let apply = self.act(
            "Apply",
            "delays",
            json!({"revision":*self.revision.borrow(),"delays":*self.draft.borrow()}).to_string(),
        );
        apply.set_sensitive(
            self.dirty.get() && i["editable"] == true && self.pending.get().is_none(),
        );
        buttons.append(&apply);
        self.body.append(&buttons);
        self.divider();
        let power = line();
        power.set_halign(gtk::Align::Center);
        power.append(&self.act("Lock now", "lock", String::new()));
        power.append(&self.act("Sleep now", "suspend", String::new()));
        let off = button("Power off now", "idle-poweroff");
        off.set_sensitive(self.pending.get().is_none());
        let weak = Rc::downgrade(self);
        off.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.confirm_power.set(true);
                let _ = c.send.try_send(Event::PanelCommand("detach".into()));
                c.render();
            }
        });
        power.append(&off);
        self.body.append(&power);
        if self.confirm_power.get() {
            self.body.append(&caption("Power off this computer now?"));
            let controls = line();
            let cancel = button("Cancel", "power-cancel");
            let weak = Rc::downgrade(self);
            cancel.connect_clicked(move |_| {
                if let Some(c) = weak.upgrade() {
                    c.confirm_power.set(false);
                    c.render();
                }
            });
            controls.append(&cancel);
            let confirm = self.act("Power off", "poweroff", "confirm".into());
            confirm.add_css_class("policy-accent");
            controls.append(&confirm);
            self.body.append(&controls);
        }
    }
    fn slider(
        self: &Rc<Self>,
        title: &str,
        key: &str,
        value: f64,
        min: f64,
        max: f64,
        step: f64,
        suffix: &str,
    ) {
        let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let row = line();
        let l = label(title, "policy-label");
        l.set_hexpand(true);
        row.append(&l);
        let value_label = label(&format!("{value:.0}{suffix}"), "policy-muted");
        row.append(&value_label);
        section.append(&row);
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step);
        scale.add_css_class("policy-scale");
        scale.set_widget_name(key);
        scale.set_draw_value(false);
        scale.set_value(value);
        scale.set_sensitive(self.pending.get().is_none());
        scale.update_property(&[gtk::accessible::Property::Label(title)]);
        let weak = Rc::downgrade(self);
        let key = key.to_owned();
        let suffix = suffix.to_owned();
        scale.connect_value_changed(move |s| {
            let v = (s.value() / step).round() * step;
            value_label.set_text(&format!("{v:.0}{suffix}"));
            if let Some(c) = weak.upgrade() {
                c.debounce(&key, format!("{v:.0}"));
            }
        });
        section.append(&scale);
        self.body.append(&section);
    }
    fn nightlight(self: &Rc<Self>) {
        let i = self.info.borrow().clone();
        let settings = &i["settings"];
        let schedule = &i["schedule"];
        let weak = Rc::downgrade(self);
        self.body.append(&self.toggle(
            "Night light",
            i["enabled"] == true,
            "nightlight-enabled",
            move |v| {
                if let Some(c) = weak.upgrade() {
                    c.run("enabled", if v { "on" } else { "off" }.into());
                }
            },
        ));
        let state = if i["enabled"] != true {
            "Off · enable to use your schedule".into()
        } else if i["running"] != true {
            "Night-light service unavailable".into()
        } else if i["active"] == true {
            "Warm colours are active".into()
        } else if schedule["polar"] == true {
            if schedule["polarNight"] == true {
                "Polar night · warm colours all day".into()
            } else {
                "Midnight sun · daylight colours all day".into()
            }
        } else {
            format!("Daylight colours · starts at {}", s(&schedule["nextTime"]))
        };
        self.body.append(&caption(&state));
        self.slider(
            "Colour temperature",
            "temperature",
            num(&settings["temperature"]),
            1000.,
            6500.,
            100.,
            " K",
        );
        let limits = line();
        let warm = caption("Warmer");
        warm.set_hexpand(true);
        limits.append(&warm);
        limits.append(&caption("Cooler"));
        self.body.append(&limits);
        self.slider(
            "Night-time brightness",
            "gamma",
            num(&settings["gamma"]),
            10.,
            150.,
            5.,
            "%",
        );
        self.divider();
        let solar = settings["mode"] == "solar";
        let weak = Rc::downgrade(self);
        self.body.append(&self.toggle(
            "Follow sunset and sunrise",
            solar,
            "nightlight-solar",
            move |v| {
                if let Some(c) = weak.upgrade() {
                    c.run("mode", if v { "solar" } else { "fixed" }.into());
                }
            },
        ));
        if solar {
            let times = line();
            let sunset = label(
                &format!("Sunset  {}", s(&schedule["sunset"])),
                "policy-label",
            );
            sunset.set_hexpand(true);
            times.append(&sunset);
            times.append(&label(
                &format!("Sunrise  {}", s(&schedule["sunrise"])),
                "policy-label",
            ));
            self.body.append(&times);
        } else {
            let row = line();
            let start = entry(s(&settings["start"]), "nightlight-start");
            start.set_max_length(5);
            let end = entry(s(&settings["end"]), "nightlight-end");
            end.set_max_length(5);
            row.append(&start);
            row.append(&caption("to"));
            row.append(&end);
            let apply = button("Apply", "nightlight-hours");
            apply.set_sensitive(self.pending.get().is_none());
            let weak = Rc::downgrade(self);
            apply.connect_clicked(move |_| {
                if let Some(c) = weak.upgrade() {
                    let valid = |s: &str| chrono::NaiveTime::parse_from_str(s, "%H:%M").is_ok();
                    if valid(&start.text()) && valid(&end.text()) && start.text() != end.text() {
                        c.run(
                            "hours",
                            json!({"start":start.text().to_string(),"end":end.text().to_string()})
                                .to_string(),
                        );
                    } else {
                        *c.feedback.borrow_mut() = "Enter different valid times as HH:MM".into();
                        c.render();
                    }
                }
            });
            row.append(&apply);
            self.body.append(&row);
        }
        self.body.append(&caption(s(&schedule["timezone"])));
        self.divider();
        let location = gtk::Box::new(gtk::Orientation::Vertical, 3);
        location.append(&caption("Location"));
        location.append(&label(
            settings["location"]["name"]
                .as_str()
                .unwrap_or("Choose a location"),
            "policy-label",
        ));
        location.set_hexpand(true);
        let row = line();
        row.append(&location);
        row.append(&self.act("Use device", "locate", String::new()));
        self.body.append(&row);
        if settings["location"]["source"] != "device" {
            self.body
                .append(&caption("A selected city stays fixed until you change it."));
        } else if num(&i["locationAgeHours"]) > 24. {
            self.body
                .append(&caption("Last device location · update when you travel"));
        }
        let row = line();
        let query = entry("", "nightlight-city-search");
        query.set_placeholder_text(Some("Search for your city…"));
        query.set_max_length(80);
        row.append(&query);
        let search = button("Search", "city-search");
        search.set_sensitive(self.pending.get().is_none());
        let weak = Rc::downgrade(self);
        let q = query.clone();
        search.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                if q.text().trim().chars().count() >= 2 {
                    c.run("city-search", q.text().trim().into());
                }
            }
        });
        let weak = Rc::downgrade(self);
        query.connect_activate(move |q| {
            if let Some(c) = weak.upgrade() {
                if q.text().trim().chars().count() >= 2 {
                    c.run("city-search", q.text().trim().into());
                }
            }
        });
        row.append(&search);
        self.body.append(&row);
        for city in arr(&i["cities"]) {
            let b = self.act(
                &format!("{}\n{}", s(&city["name"]), s(&city["detail"])),
                "city-select",
                st(&city["id"]),
            );
            b.add_css_class("policy-city");
            self.body.append(&b);
        }
    }
    fn edit_monitor(self: &Rc<Self>, name: &str, patch: Value) {
        let mut draft = self.draft.borrow_mut();
        let Some(m) = draft.as_array_mut().and_then(|v| {
            v.iter_mut().find(|m| {
                s(&m["name"]) == name && m["connected"] == true && m["configured"] == true
            })
        }) else {
            return;
        };
        let mut changed = false;
        for (k, v) in patch.as_object().into_iter().flatten() {
            if m["settings"][k] != *v {
                m["settings"][k] = v.clone();
                changed = true;
            }
        }
        if !changed {
            return;
        }
        if patch.get("mode").is_some()
            && !geometry::scales(m).contains(&num(&m["settings"]["scale"]))
        {
            m["settings"]["scale"] = json!(1.);
        }
        drop(draft);
        self.dirty.set(
            !arr(&geometry::changes(
                &self.basis.borrow(),
                &self.draft.borrow(),
            ))
            .is_empty(),
        );
        self.render();
    }
    fn enabled(self: &Rc<Self>, name: &str, on: bool) {
        if !on {
            let mut draft = self.draft.borrow_mut();
            for m in draft.as_array_mut().into_iter().flatten() {
                if m["settings"]["mirror"] == name {
                    m["settings"]["mirror"] = json!("");
                }
            }
        }
        self.edit_monitor(name, json!({"enabled":on}));
    }
    fn monitor_choice(
        self: &Rc<Self>,
        body: &gtk::Box,
        current: &Value,
        title: &str,
        key: &'static str,
        values: Vec<(Value, String)>,
    ) {
        let weak = Rc::downgrade(self);
        let name = s(&current["name"]).to_owned();
        let c = choice(
            values,
            &current["settings"][key],
            &format!("display-{key}"),
            move |v| {
                if let Some(c) = weak.upgrade() {
                    let mut patch = json!({});
                    patch[key] = v;
                    c.edit_monitor(&name, patch);
                }
            },
        );
        pair(body, title, &c);
    }
    fn monitors(self: &Rc<Self>) {
        let info = self.info.borrow().clone();
        let draft = self.draft.borrow().clone();
        let selected = self.selected.borrow().clone();
        let busy = self.pending.get().is_some() || !self.change.borrow().is_null();
        if !self.change.borrow().is_null() {
            let change = self.change.borrow().clone();
            let confirm = gtk::Box::new(gtk::Orientation::Vertical, 8);
            confirm.add_css_class("policy-confirm");
            confirm.append(&label(
                if change["phase"] == "confirm" {
                    "Keep these display settings?"
                } else {
                    "Restoring / applying displays…"
                },
                "policy-label",
            ));
            confirm.append(&self.countdown);
            let buttons = line();
            for (decision, title) in [("revert", "Revert"), ("keep", "Keep changes")] {
                let b = button(title, decision);
                b.set_sensitive(
                    decision == "revert"
                        || (change["phase"] == "confirm"
                            && num(&change["deadline"])
                                > chrono::Local::now().timestamp_millis() as f64 / 1000.),
                );
                let token = change["token"].clone();
                let send = self.send.clone();
                b.connect_clicked(move |_| {
                    let _ = send.try_send(Event::SystemAction(
                        "monitor-layout".into(),
                        json!({"op":"monitorDecision","decision":decision,"token":token}),
                    ));
                });
                buttons.append(&b);
            }
            confirm.append(&buttons);
            self.body.append(&confirm);
        }
        let presets = line();
        let title = label("Display arrangement", "policy-label");
        title.set_hexpand(true);
        presets.append(&title);
        for (action, title) in [
            ("desktop", "Desktop"),
            ("game", "Game / TV"),
            ("mirror", "TV mirror"),
        ] {
            let b = button(title, &format!("display-{action}"));
            let tv = s(&info["tv"]).to_owned();
            let copy = s(&info["mirrorOutput"]).to_owned();
            let available = |name: &str| {
                arr(&draft)
                    .iter()
                    .any(|m| m["name"] == name && m["connected"] == true && m["configured"] == true)
            };
            b.set_sensitive(!busy && available(&tv) && (action != "mirror" || available(&copy)));
            let weak = Rc::downgrade(self);
            b.connect_clicked(move |_| {
                if let Some(c) = weak.upgrade() {
                    *c.selected.borrow_mut() = tv.clone();
                    c.enabled(&tv, action != "desktop");
                    c.edit_monitor(&tv, json!({"mirror":""}));
                    if action == "mirror" {
                        *c.selected.borrow_mut() = copy.clone();
                        c.edit_monitor(&copy, json!({"enabled":true,"mirror":tv}));
                    }
                }
            });
            presets.append(&b);
        }
        self.body.append(&presets);
        self.body.append(&self.map());
        self.body.append(&caption(
            "Select a display · drag to arrange · sizes reflect resolution and scaling",
        ));
        let selectors = gtk::FlowBox::new();
        selectors.set_selection_mode(gtk::SelectionMode::None);
        selectors.set_column_spacing(6);
        selectors.set_row_spacing(6);
        for m in arr(&draft) {
            let name = s(&m["name"]).to_owned();
            let b = button(&name, "display-select");
            if name == selected {
                b.add_css_class("policy-accent");
            }
            let weak = Rc::downgrade(self);
            b.connect_clicked(move |_| {
                if let Some(c) = weak.upgrade() {
                    *c.selected.borrow_mut() = name.clone();
                    c.render();
                }
            });
            selectors.insert(&b, -1);
        }
        self.body.append(&selectors);
        if let Some(m) = arr(&draft).iter().find(|m| s(&m["name"]) == selected) {
            let editable = m["connected"] == true && m["configured"] == true && !busy;
            let row = line();
            let names = gtk::Box::new(gtk::Orientation::Vertical, 3);
            names.set_hexpand(true);
            names.append(&label(
                m["label"].as_str().unwrap_or(&selected),
                "policy-title",
            ));
            names.append(&caption(if m["connected"] != true {
                "Disconnected · last configured position"
            } else if m["configured"] != true {
                "Set up this display in the compositor first"
            } else {
                "Connected"
            }));
            row.append(&names);
            let weak = Rc::downgrade(self);
            let name = selected.clone();
            let toggle = self.toggle(
                "",
                m["settings"]["enabled"] == true,
                "display-enabled",
                move |v| {
                    if let Some(c) = weak.upgrade() {
                        c.enabled(&name, v);
                    }
                },
            );
            toggle.set_sensitive(editable);
            row.append(&toggle);
            self.body.append(&row);
            let controls = gtk::Box::new(gtk::Orientation::Vertical, 8);
            controls.set_sensitive(editable);
            let mut mirrors = vec![(json!(""), "Extend desktop".into())];
            if !arr(&draft).iter().any(|p| {
                p["connected"] == true
                    && p["settings"]["enabled"] == true
                    && p["settings"]["mirror"] == selected
            }) {
                mirrors.extend(
                    arr(&draft)
                        .iter()
                        .filter(|p| {
                            p["name"] != selected
                                && p["connected"] == true
                                && p["settings"]["enabled"] == true
                                && s(&p["settings"]["mirror"]).is_empty()
                        })
                        .map(|p| (p["name"].clone(), format!("Mirror {}", s(&p["name"])))),
                );
            }
            self.monitor_choice(&controls, m, "Use as", "mirror", mirrors);
            self.monitor_choice(
                &controls,
                m,
                "Resolution / refresh",
                "mode",
                arr(&m["availableModes"])
                    .iter()
                    .map(|v| (v.clone(), format!("{} Hz", s(v).replace('@', " · "))))
                    .collect(),
            );
            self.monitor_choice(
                &controls,
                m,
                "Scale",
                "scale",
                geometry::scales(m)
                    .into_iter()
                    .map(|v| (json!(v), format!("{:.0}%", v * 100.)))
                    .collect(),
            );
            self.monitor_choice(
                &controls,
                m,
                "Rotation",
                "transform",
                [
                    (0, "Landscape"),
                    (1, "Portrait · 90°"),
                    (2, "Upside down · 180°"),
                    (3, "Portrait · 270°"),
                ]
                .into_iter()
                .map(|(v, l)| (json!(v), l.into()))
                .collect(),
            );
            let positions = line();
            for key in ["x", "y"] {
                let e = entry(&st(&m["settings"][key]), &format!("display-position-{key}"));
                e.set_hexpand(false);
                e.set_width_chars(4);
                e.set_max_width_chars(4);
                e.set_sensitive(s(&m["settings"]["mirror"]).is_empty());
                let weak = Rc::downgrade(self);
                let name = selected.clone();
                e.connect_activate(move |e| {
                    if let Some(c) = weak.upgrade() {
                        if let Ok(v) = e.text().parse::<i32>() {
                            if (-32768..=32768).contains(&v) {
                                let mut p = json!({});
                                p[key] = json!(v);
                                c.edit_monitor(&name, p);
                            }
                        }
                    }
                });
                positions.append(&e);
            }
            pair(&controls, "X / Y · Enter", &positions);
            positions.set_size_request(-1, -1);
            if m["hardwareDetails"]["vrrCapable"] == true {
                self.monitor_choice(
                    &controls,
                    m,
                    "Adaptive sync",
                    "vrr",
                    [
                        (-1, "Use compositor setting"),
                        (0, "Off"),
                        (2, "Fullscreen only"),
                        (1, "Always"),
                    ]
                    .into_iter()
                    .map(|(v, l)| (json!(v), l.into()))
                    .collect(),
                );
            }
            if m["hardwareDetails"]["hdr"] == true {
                self.monitor_choice(
                    &controls,
                    m,
                    "Colour mode",
                    "cm",
                    [
                        ("auto", "Automatic"),
                        ("srgb", "Standard · sRGB"),
                        ("hdr", "HDR"),
                    ]
                    .into_iter()
                    .map(|(v, l)| (json!(v), l.into()))
                    .collect(),
                );
            }
            self.body.append(&controls);
        }
        let usable = arr(&draft).iter().any(|m| {
            m["connected"] == true
                && m["settings"]["enabled"] == true
                && s(&m["settings"]["mirror"]).is_empty()
        });
        let footer = line();
        footer.set_halign(gtk::Align::End);
        let reset = button("Reset", "display-reset");
        reset.set_sensitive(self.dirty.get() && !busy);
        let weak = Rc::downgrade(self);
        reset.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.reset_draft();
                c.render();
            }
        });
        footer.append(&reset);
        let apply=self.act("Apply","apply",json!({"revision":*self.revision.borrow(),"edits":geometry::changes(&self.basis.borrow(),&draft)}).to_string());
        apply.set_sensitive(self.dirty.get() && usable && !busy);
        let send = self.send.clone();
        apply.connect_clicked(move |_| {
            let _ = send.try_send(Event::PanelCommand("detach".into()));
        });
        footer.append(&apply);
        self.body.append(&footer);
    }
    fn map(self: &Rc<Self>) -> gtk::DrawingArea {
        let area = gtk::DrawingArea::new();
        area.set_widget_name("display-map");
        area.add_css_class("panel-interactive");
        area.add_css_class("policy-map");
        area.set_content_height(200);
        area.set_hexpand(true);
        let viewport = Rc::new(Cell::new(geometry::bounds(arr(&self.draft.borrow()))));
        let view = viewport.clone();
        let weak = Rc::downgrade(self);
        area.set_draw_func(move |_, cr, w, h| {
            let Some(c) = weak.upgrade() else {
                return;
            };
            let data = c.draft.borrow();
            let all = arr(&data);
            let (scale, ox, oy) = geometry::fit(view.get(), w as f64, h as f64);
            let style = c.style.borrow();
            let ink = |key: &str, fallback: &str| {
                gtk::gdk::RGBA::parse(s(&style[key]))
                    .unwrap_or_else(|_| gtk::gdk::RGBA::parse(fallback).unwrap())
            };
            let background = ink("background", "#1f1d2e");
            let accent = ink("accent", "#ff53a6");
            let text = ink("text", "#e5e5e5");
            let border = ink("border", "#555169");
            for (index, m) in all.iter().enumerate() {
                if geometry::mirrored(m, all) {
                    continue;
                }
                let (mw, mh) = geometry::size(m);
                let (x, y) = (
                    ox + num(&m["settings"]["x"]) * scale,
                    oy + num(&m["settings"]["y"]) * scale,
                );
                let (w, h) = ((mw * scale).max(18.), (mh * scale).max(18.));
                let selected = s(&m["name"]) == *c.selected.borrow()
                    || all.iter().any(|p| {
                        s(&p["name"]) == *c.selected.borrow()
                            && p["settings"]["mirror"] == m["name"]
                    });
                let alpha = if m["connected"] != true {
                    0.4
                } else if m["settings"]["enabled"] != true {
                    0.65
                } else {
                    1.
                };
                let radius = 4_f64.min(w / 2.).min(h / 2.);
                cr.new_sub_path();
                for (cx, cy, start) in [
                    (x + w - radius, y + radius, -90_f64),
                    (x + w - radius, y + h - radius, 0.),
                    (x + radius, y + h - radius, 90.),
                    (x + radius, y + radius, 180.),
                ] {
                    cr.arc(
                        cx,
                        cy,
                        radius,
                        start.to_radians(),
                        (start + 90.).to_radians(),
                    );
                }
                cr.close_path();
                cr.set_source_rgba(
                    background.red() as f64,
                    background.green() as f64,
                    background.blue() as f64,
                    alpha,
                );
                let _ = cr.fill_preserve();
                if selected {
                    cr.set_source_rgba(
                        accent.red() as f64,
                        accent.green() as f64,
                        accent.blue() as f64,
                        0.14,
                    );
                    let _ = cr.fill_preserve();
                }
                let color = if selected { accent } else { border };
                cr.set_source_rgba(
                    color.red() as f64,
                    color.green() as f64,
                    color.blue() as f64,
                    alpha,
                );
                cr.set_line_width(if selected { 2. } else { 1. });
                let _ = cr.stroke();
                let _ = cr.save();
                cr.rectangle(x + 3., y + 3., (w - 6.).max(0.), (h - 6.).max(0.));
                cr.clip();
                cr.set_source_rgba(
                    text.red() as f64,
                    text.green() as f64,
                    text.blue() as f64,
                    alpha,
                );
                cr.set_font_size(11.);
                let title = format!("{} · {}", index + 1, s(&m["name"]));
                let width = cr.text_extents(&title).map(|t| t.width()).unwrap_or(0.);
                cr.move_to(x + (w - width) / 2., y + h / 2. - 2.);
                let _ = cr.show_text(&title);
                let status = if m["connected"] != true {
                    "Disconnected"
                } else if m["settings"]["enabled"] != true {
                    "Off"
                } else {
                    "On"
                };
                let width = cr.text_extents(status).map(|t| t.width()).unwrap_or(0.);
                cr.move_to(x + (w - width) / 2., y + h / 2. + 13.);
                let _ = cr.show_text(status);
                let _ = cr.restore();
            }
        });
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        let origin = Rc::new(RefCell::new(None::<(String, f64, f64, f64)>));
        let o = origin.clone();
        let weak = Rc::downgrade(self);
        let a = area.downgrade();
        let v = viewport.clone();
        drag.connect_drag_begin(move |g, x, y| {
            let (Some(c), Some(a)) = (weak.upgrade(), a.upgrade()) else {
                return;
            };
            let data = c.draft.borrow();
            let all = arr(&data);
            let (scale, ox, oy) = geometry::fit(v.get(), a.width() as f64, a.height() as f64);
            for m in all.iter().rev().filter(|m| !geometry::mirrored(m, all)) {
                let (w, h) = geometry::size(m);
                let (mx, my) = (num(&m["settings"]["x"]), num(&m["settings"]["y"]));
                if x >= ox + mx * scale
                    && x <= ox + (mx + w) * scale
                    && y >= oy + my * scale
                    && y <= oy + (my + h) * scale
                {
                    *c.selected.borrow_mut() = s(&m["name"]).into();
                    if m["connected"] == true
                        && m["configured"] == true
                        && c.pending.get().is_none()
                        && c.change.borrow().is_null()
                    {
                        *o.borrow_mut() = Some((s(&m["name"]).into(), mx, my, scale));
                    }
                    g.set_state(gtk::EventSequenceState::Claimed);
                    a.queue_draw();
                    break;
                }
            }
        });
        let o = origin.clone();
        let weak = Rc::downgrade(self);
        let a = area.downgrade();
        drag.connect_drag_update(move |_, dx, dy| {
            let (Some(c), Some(a)) = (weak.upgrade(), a.upgrade()) else {
                return;
            };
            let origin = o.borrow();
            let Some((name, x, y, scale)) = origin.as_ref() else {
                return;
            };
            let snapshot = c.draft.borrow().clone();
            let Some(m) = arr(&snapshot).iter().find(|m| s(&m["name"]) == name) else {
                return;
            };
            let (x, y) = geometry::snap(
                m,
                arr(&snapshot),
                x + dx / scale,
                y + dy / scale,
                10. / scale,
            );
            for m in c.draft.borrow_mut().as_array_mut().into_iter().flatten() {
                if s(&m["name"]) == name {
                    m["settings"]["x"] = json!(x as i64);
                    m["settings"]["y"] = json!(y as i64);
                }
            }
            c.dirty
                .set(!arr(&geometry::changes(&c.basis.borrow(), &c.draft.borrow())).is_empty());
            a.queue_draw();
        });
        let weak = Rc::downgrade(self);
        drag.connect_drag_end(move |_, _, _| {
            origin.borrow_mut().take();
            if let Some(c) = weak.upgrade() {
                c.render();
            }
        });
        area.add_controller(drag);
        area
    }
}

#[cfg(test)]
pub fn fixture(provider: &str) -> Value {
    serde_json::from_str(match provider {
        "caffeine" => include_str!("../../tests/fixtures/caffeine-card.json"),
        "nightlight" => include_str!("../../tests/fixtures/nightlight-card.json"),
        _ => include_str!("../../tests/fixtures/monitor-layout-card.json"),
    })
    .unwrap()
}
#[cfg(test)]
pub fn exercise_fixture(c: &Rc<Card>, events: &async_channel::Receiver<Event>) {
    fn find(w: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
        if w.widget_name() == name {
            return Some(w.clone());
        }
        let mut child = w.first_child();
        while let Some(w) = child {
            if let Some(found) = find(&w, name) {
                return Some(found);
            }
            child = w.next_sibling();
        }
        None
    }
    let get =
        |name: &str| find(c.body.upcast_ref(), name).unwrap_or_else(|| panic!("missing {name}"));
    let next = || loop {
        match events.try_recv().expect("missing intent") {
            Event::SystemAction(_, v) => break v,
            _ => {}
        }
    };
    let ack = |request: &Value| {
        c.acknowledge(&json!({"requestId":request["context"]["requestId"],"barKey":"fixture","module":c.provider,"busy":false,"error":false,"message":"Saved"}));
    };
    match c.provider {
        "caffeine" => {
            get("idle-delay-dim")
                .downcast::<gtk::DropDown>()
                .unwrap()
                .set_selected(1);
            assert!(c.dirty.get());
            get("delays")
                .downcast::<gtk::Button>()
                .unwrap()
                .emit_clicked();
            let request = next();
            let value: Value = serde_json::from_str(s(&request["context"]["selection"])).unwrap();
            assert_eq!(value["revision"], "synthetic-idle");
            assert_eq!(value["delays"]["dim"], 15);
            ack(&request);
            assert!(c.feedback.borrow().is_empty());
            assert_eq!(
                get("idle-duration").width_request(),
                get("idle-delay-dim").width_request()
            );
            while events.try_recv().is_ok() {}
            get("idle-poweroff")
                .downcast::<gtk::Button>()
                .unwrap()
                .emit_clicked();
            assert!(c.confirm_power.get());
            while let Ok(e) = events.try_recv() {
                assert!(!matches!(e, Event::SystemAction(..)));
            }
            get("power-cancel")
                .downcast::<gtk::Button>()
                .unwrap()
                .emit_clicked();
            assert!(!c.confirm_power.get());
        }
        "nightlight" => {
            let slider = get("temperature").downcast::<gtk::Scale>().unwrap();
            for v in 20..40 {
                slider.set_value(v as f64 * 100.);
            }
            assert_eq!(c.timers.borrow().len(), 1);
            let until = std::time::Instant::now() + Duration::from_millis(450);
            while std::time::Instant::now() < until {
                while glib::MainContext::default().pending() {
                    glib::MainContext::default().iteration(false);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let request = next();
            assert_eq!(request["context"]["action"], "temperature");
            assert_eq!(request["context"]["selection"], "3900");
            c.acknowledge(
                &json!({"requestId":999,"module":c.provider,"barKey":"fixture","busy":false}),
            );
            assert!(c.pending.get().is_some());
            ack(&request);
            assert!(c.pending.get().is_none());
            let mut data = fixture("nightlight");
            data["settings"]["mode"] = json!("fixed");
            let context = c.context.borrow().clone();
            let style = c.style.borrow().clone();
            c.update(data, context, style);
            assert!(find(c.body.upcast_ref(), "nightlight-start").is_some());
        }
        _ => {
            c.edit_monitor("DP-1", json!({"x":-120,"transform":1}));
            assert!(c.dirty.get());
            get("apply")
                .downcast::<gtk::Button>()
                .unwrap()
                .emit_clicked();
            let request = next();
            let value: Value = serde_json::from_str(s(&request["context"]["selection"])).unwrap();
            assert_eq!(value["revision"], "synthetic-monitors");
            assert_eq!(value["edits"][0]["settings"]["x"], -120);
            ack(&request);
            c.monitor_change(json!({"token":"fixture-token","phase":"confirm","deadline":chrono::Local::now().timestamp()+20}));
            get("keep")
                .downcast::<gtk::Button>()
                .unwrap()
                .emit_clicked();
            let decision = next();
            assert_eq!(
                decision,
                json!({"op":"monitorDecision","decision":"keep","token":"fixture-token"})
            );
            c.monitor_change(Value::Null);
            assert!(!c.dirty.get());
            for name in ["DP-1", "DP-2", "HDMI-A-1"] {
                c.enabled(name, false);
            }
            assert!(!get("apply").is_sensitive());
        }
    }
}
