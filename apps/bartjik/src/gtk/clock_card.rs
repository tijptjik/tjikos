//! Datetime presentation only: provider reads are owned by clocks::Service.
use crate::{
    clocks::Reading,
    config,
    ui::{full_input, surface},
    util::{arr, s},
};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use tjik_gtk::gtk::{self, gdk, glib, prelude::*};
pub struct Card {
    pub button: gtk::Button,
    pub panel: gtk::ApplicationWindow,
    pub module: Value,
    pub key: String,
    rows: gtk::Box,
    alternate: Cell<bool>,
    layout_dirty: Cell<bool>,
    reading: RefCell<Option<Reading>>,
    rendered: RefCell<Value>,
    hover_epoch: Cell<u64>,
}
impl Card {
    pub fn new(app: &gtk::Application, monitor: &gdk::Monitor, module: &Value) -> Rc<Self> {
        let panel = surface(app, monitor, "bartjik-gtk-clock", Layer::Overlay);
        panel.set_keyboard_mode(KeyboardMode::OnDemand);
        panel.connect_map(full_input);
        let pane = gtk::Box::new(gtk::Orientation::Vertical, 12);
        pane.add_css_class("pane");
        let heading = gtk::Label::new(Some("World clock"));
        heading.set_xalign(0.);
        heading.add_css_class("clock-heading");
        pane.append(&heading);
        pane.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let rows = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(80)
            .max_content_height(420)
            .propagate_natural_height(true)
            .child(&rows)
            .build();
        pane.append(&scroll);
        let footer = gtk::Label::new(Some("Local times · daylight saving aware"));
        footer.set_xalign(0.);
        footer.add_css_class("clock-detail");
        pane.append(&footer);
        let close = gtk::Button::with_label("Close");
        pane.append(&close);
        panel.set_child(Some(&pane));
        let button = gtk::Button::with_label("Loading…");
        button.set_tooltip_text(Some("Loading clock"));
        let card = Rc::new(Self {
            button,
            panel,
            module: module.clone(),
            key: config::provider_key(module),
            rows,
            alternate: Cell::new(false),
            layout_dirty: Cell::new(true),
            reading: RefCell::new(None),
            rendered: RefCell::new(Value::Null),
            hover_epoch: Cell::new(0),
        });
        let weak = Rc::downgrade(&card);
        card.button.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.alternate.set(!c.alternate.get());
                c.render();
            }
        });
        let right = gtk::GestureClick::new();
        right.set_button(3);
        let weak = Rc::downgrade(&card);
        right.connect_released(move |_, _, _, _| {
            if let Some(c) = weak.upgrade() {
                c.show();
            }
        });
        card.button.add_controller(right);
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&card);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            if key == gdk::Key::Menu
                || (key == gdk::Key::F10 && modifiers.contains(gdk::ModifierType::SHIFT_MASK))
            {
                if let Some(c) = weak.upgrade() {
                    c.show();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        card.button.add_controller(keys);
        let weak = Rc::downgrade(&card);
        close.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.hide();
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&card);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(c) = weak.upgrade() {
                    c.hide();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        card.panel.add_controller(keys);
        card.panel.connect_close_request(|p| {
            p.set_visible(false);
            glib::Propagation::Stop
        });
        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(&card);
        motion.connect_enter(move |_, _, _| {
            if let Some(c) = weak.upgrade() {
                if c.module["panel"]["hover"] == true {
                    let delay = c.module["panel"]["delay"].as_u64().unwrap_or(300);
                    c.schedule(true, delay);
                }
            }
        });
        let weak = Rc::downgrade(&card);
        motion.connect_leave(move |_| {
            if let Some(c) = weak.upgrade() {
                if c.module["panel"]["hover"] == true {
                    c.schedule(
                        false,
                        c.module["panel"]["closeDelay"]
                            .as_u64()
                            .unwrap_or(250)
                            .max(250),
                    );
                }
            }
        });
        card.button.add_controller(motion);
        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(&card);
        motion.connect_enter(move |_, _, _| {
            if let Some(c) = weak.upgrade() {
                c.hover_epoch.set(c.hover_epoch.get() + 1);
            }
        });
        let weak = Rc::downgrade(&card);
        motion.connect_leave(move |_| {
            if let Some(c) = weak.upgrade() {
                if c.module["panel"]["hover"] == true {
                    c.schedule(
                        false,
                        c.module["panel"]["closeDelay"]
                            .as_u64()
                            .unwrap_or(250)
                            .max(250),
                    );
                }
            }
        });
        card.panel.add_controller(motion);
        card
    }
    fn schedule(self: &Rc<Self>, show: bool, delay: u64) {
        let epoch = self.hover_epoch.get() + 1;
        self.hover_epoch.set(epoch);
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(delay), move || {
            if let Some(c) = weak.upgrade() {
                if c.hover_epoch.get() == epoch {
                    if show {
                        c.show();
                    } else {
                        c.hide();
                    }
                }
            }
        });
    }
    fn show(&self) {
        self.hover_epoch.set(self.hover_epoch.get() + 1);
        self.render();
        self.panel.set_visible(true);
    }
    pub fn hide(&self) {
        self.hover_epoch.set(self.hover_epoch.get() + 1);
        self.panel.set_visible(false);
    }
    pub fn update(&self, reading: Option<&Reading>) -> bool {
        let before = self.button.label();
        *self.reading.borrow_mut() = reading.cloned();
        self.render();
        self.layout_dirty.replace(false) || before != self.button.label()
    }
    fn render(&self) {
        let (label, tip, rows) = match self.reading.borrow().as_ref() {
            Some(Reading::Ready(v)) => (
                if self.alternate.get() {
                    v.alternate.clone()
                } else {
                    v.text.clone()
                },
                v.tooltip.clone(),
                v.panel["rows"].clone(),
            ),
            Some(Reading::Error(e)) => (
                "Clock unavailable".into(),
                e.clone(),
                json!([{"label":"Clock unavailable","detail":e,"value":""}]),
            ),
            None => (
                "Loading…".into(),
                "Loading clock".into(),
                json!([{"label":"Loading…","detail":"","value":""}]),
            ),
        };
        if self.button.label().as_deref() != Some(label.as_str()) {
            self.layout_dirty.set(true);
        }
        self.button.set_label(&label);
        self.button.set_tooltip_text(Some(&format!(
            "{tip}\nClick to change format · Right-click for world clock"
        )));
        if *self.rendered.borrow() == rows {
            return;
        }
        while let Some(child) = self.rows.first_child() {
            self.rows.remove(&child);
        }
        if arr(&rows).is_empty() {
            self.rows
                .append(&gtk::Label::new(Some("No world clocks configured")));
        }
        for row in arr(&rows) {
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            line.add_css_class("clock-row");
            let text = gtk::Box::new(gtk::Orientation::Vertical, 4);
            text.set_hexpand(true);
            for (key, class) in [("label", "clock-city"), ("detail", "clock-detail")] {
                let label = gtk::Label::new(Some(s(&row[key])));
                label.set_xalign(0.);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                label.add_css_class(class);
                text.append(&label);
            }
            line.append(&text);
            let value = gtk::Label::new(Some(s(&row["value"])));
            value.add_css_class("clock-value");
            line.append(&value);
            self.rows.append(&line);
        }
        *self.rendered.borrow_mut() = rows;
    }
    pub fn status(&self) -> Value {
        json!({"text":self.button.label().map(|s|s.to_string()),"alternate":self.alternate.get(),"panel":self.panel.is_visible(),"rows":*self.rendered.borrow(),"state":match self.reading.borrow().as_ref(){None=>"loading",Some(Reading::Ready(_))=>"ready",Some(Reading::Error(_))=>"error"}})
    }
}
impl Drop for Card {
    fn drop(&mut self) {
        self.panel.destroy();
    }
}
