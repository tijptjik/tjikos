//! Combined VPN presentation. Existing core providers own tools, policy and actions.
use crate::{
    panel_view::Content,
    util::{arr, s},
    Event,
};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use tjik_gtk::gtk::{self, glib, prelude::*};
fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    l.add_css_class(class);
    l
}
fn icon(text: &str) -> gtk::Label {
    crate::panel_view::control_symbol(text)
}
fn column(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}
fn row() -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, 8)
}
fn divider() -> gtk::Separator {
    gtk::Separator::new(gtk::Orientation::Horizontal)
}
fn flag(country: &str, size: i32) -> gtk::Widget {
    if country.len() == 2 && country.bytes().all(|b| b.is_ascii_lowercase()) {
        let path = crate::util::root()
            .join("qml/icons/flags")
            .join(format!("{country}.svg"));
        if path.is_file() {
            if size <= 32 {
                let image = gtk::Image::from_file(path);
                image.set_pixel_size(size);
                image.set_valign(gtk::Align::Center);
                return image.upcast();
            }
            let image = gtk::Picture::for_filename(path);
            image.set_can_shrink(true);
            image.set_size_request(size, size * 3 / 4);
            image.set_opacity(0.84);
            return image.upcast();
        }
    }
    icon("public").upcast()
}
pub struct Card {
    pub body: gtk::Box,
    data: RefCell<Value>,
    expanded: Cell<bool>,
    pending: Cell<Option<u64>>,
    sequence: Cell<u64>,
    timer: RefCell<Option<glib::SourceId>>,
    feedback: RefCell<String>,
    send: async_channel::Sender<Event>,
}
impl Drop for Card {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.get_mut().take() {
            timer.remove();
        }
    }
}
impl Card {
    pub fn new(send: async_channel::Sender<Event>) -> Rc<Self> {
        let card = Rc::new(Self {
            body: column(12),
            data: RefCell::new(Value::Null),
            expanded: Cell::new(false),
            pending: Cell::new(None),
            sequence: Cell::new(0),
            timer: RefCell::new(None),
            feedback: RefCell::new(String::new()),
            send,
        });
        let weak = Rc::downgrade(&card);
        card.body.connect_unmap(move |_| {
            if let Some(c) = weak.upgrade() {
                c.expanded.set(false);
            }
        });
        card
    }
    pub fn content(&self) -> Content {
        Content {
            title: if self.data.borrow()["sections"][0]["provider"] == "tailscale" {
                "Tailscale"
            } else {
                "Mullvad"
            }
            .into(),
            body: self.body.clone().upcast(),
            footer: None,
            preferred_width: 420.,
            needs_keyboard: false,
        }
    }
    pub fn update(self: &Rc<Self>, data: Value) {
        if *self.data.borrow() == data && self.body.is_mapped() {
            return;
        }
        if self.data.borrow()["sections"][0]["row"]["panel"]["countryCode"]
            != data["sections"][0]["row"]["panel"]["countryCode"]
        {
            self.expanded.set(false);
        }
        *self.data.borrow_mut() = data;
        self.render();
    }
    pub fn acknowledge(self: &Rc<Self>, feedback: &Value) {
        if feedback["requestId"].as_u64() == self.pending.get()
            && self.pending.get().is_some()
            && feedback["busy"] == false
        {
            self.finish(s(&feedback["message"]));
        }
    }
    pub fn finish(self: &Rc<Self>, message: &str) {
        self.pending.set(None);
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.remove();
        }
        *self.feedback.borrow_mut() = message.into();
        self.render();
        self.resized();
    }
    fn resized(&self) {
        let _ = self.send.try_send(Event::PanelCommand("resize".into()));
    }
    fn request(self: &Rc<Self>, context: Value, action: &str, selection: Value) {
        if self.pending.get().is_some() {
            return;
        }
        if action == "city" {
            self.expanded.set(false);
        }
        let id = self.sequence.get() + 1;
        self.sequence.set(id);
        self.pending.set(Some(id));
        let mut context = context;
        context["action"] = json!(action);
        context["selection"] = selection;
        context["requestId"] = json!(id);
        if self
            .send
            .try_send(Event::ConnectivityAction(context))
            .is_err()
        {
            self.finish("Backend command queue is unavailable");
            return;
        }
        self.feedback.borrow_mut().clear();
        self.render();
        self.resized();
        let weak = Rc::downgrade(self);
        *self.timer.borrow_mut() = Some(glib::timeout_add_local_once(
            std::time::Duration::from_secs(30),
            move || {
                if let Some(c) = weak.upgrade() {
                    c.timer.borrow_mut().take();
                    if c.pending.get() == Some(id) {
                        c.finish("Request timed out; waiting for current service state");
                    }
                }
            },
        ));
    }
    fn button(
        self: &Rc<Self>,
        symbol: &str,
        name: &str,
        context: &Value,
        action: &str,
        selection: Value,
        enabled: bool,
    ) -> gtk::Button {
        let b = gtk::Button::new();
        b.set_widget_name(name);
        b.set_valign(gtk::Align::Center);
        b.add_css_class("panel-control");
        b.set_child(Some(&icon(symbol)));
        b.update_property(&[gtk::accessible::Property::Label(name)]);
        b.set_sensitive(enabled && self.pending.get().is_none());
        let weak = Rc::downgrade(self);
        let context = context.clone();
        let action = action.to_owned();
        b.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.request(context.clone(), &action, selection.clone());
            }
        });
        b
    }
    fn render(self: &Rc<Self>) {
        while let Some(w) = self.body.first_child() {
            self.body.remove(&w);
        }
        let data = self.data.borrow().clone();
        for (index, section) in arr(&data["sections"]).iter().enumerate() {
            let provider = s(&section["provider"]);
            let context = &section["context"];
            let panel = &section["row"]["panel"];
            if index > 0 {
                let header = row();
                header.set_height_request(44);
                let image =
                    gtk::Image::from_file(crate::util::root().join("qml/icons/tailscale.svg"));
                image.set_pixel_size(22);
                header.append(&image);
                header.append(&label("Tailscale", "panel-heading"));
                self.body.append(&header);
                self.body.append(&divider());
            }
            if section["row"]["state"]["stale"] == true {
                self.body
                    .append(&label("Cached · waiting for an update", "network-muted"));
            }
            if provider == "mullvad" {
                self.mullvad(panel, context);
            } else {
                self.tailscale(panel, context);
            }
        }
        let message = if self.pending.get().is_some() {
            "Applying…".into()
        } else {
            self.feedback.borrow().clone()
        };
        if !message.is_empty() {
            let l = label(&message, "network-muted");
            l.set_ellipsize(gtk::pango::EllipsizeMode::None);
            l.set_wrap(true);
            self.body.append(&l);
        }
    }
    fn mullvad(self: &Rc<Self>, info: &Value, context: &Value) {
        let card = column(12);
        card.add_css_class("network-connection");
        let top = row();
        let names = column(4);
        names.set_hexpand(true);
        let connected = info["connection"] == "connected";
        names.append(&label(
            match s(&info["connection"]) {
                "connected" => "Connected",
                "connecting" => "Connecting…",
                "disconnected" => "Disconnected",
                _ => "Unavailable",
            },
            "network-title",
        ));
        names.append(&label(
            if connected {
                "Traffic protected by Mullvad"
            } else {
                "VPN connection"
            },
            "network-muted",
        ));
        top.append(&names);
        let power = self.button(
            "power_settings_new",
            if connected {
                "Disconnect Mullvad"
            } else {
                "Connect Mullvad"
            },
            context,
            if connected { "disconnect" } else { "connect" },
            Value::Null,
            !s(&info["connection"]).is_empty(),
        );
        if connected {
            power.add_css_class("network-accent");
        }
        top.append(&power);
        card.append(&top);
        card.append(&divider());
        let location = row();
        location.append(&flag(s(&info["countryCode"]), 32));
        let names = column(3);
        names.set_hexpand(true);
        names.append(&label(
            info["country"].as_str().unwrap_or("Choose a country"),
            "vpn-country",
        ));
        names.append(&label(
            info["city"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("Select a location below"),
            "vpn-city",
        ));
        location.append(&names);
        let expand = gtk::Button::new();
        expand.set_widget_name("mullvad-cities");
        expand.add_css_class("panel-control");
        expand.set_child(Some(&icon(if self.expanded.get() {
            "expand_more"
        } else {
            "chevron_right"
        })));
        expand.update_property(&[gtk::accessible::Property::Label(
            "Show or hide Mullvad cities",
        )]);
        expand.set_sensitive(!arr(&info["cities"]).is_empty() && self.pending.get().is_none());
        let weak = Rc::downgrade(self);
        expand.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.expanded.set(!c.expanded.get());
                c.render();
                c.resized();
            }
        });
        location.append(&expand);
        card.append(&location);
        if self.expanded.get() {
            let cities = column(4);
            for city in arr(&info["cities"]).iter().take(128) {
                let b = self.button(
                    "",
                    s(&city["name"]),
                    context,
                    "city",
                    json!({"country":info["countryCode"],"city":city["id"]}),
                    city["active"] != true,
                );
                b.remove_css_class("panel-control");
                b.add_css_class("vpn-city-option");
                let line = row();
                let name = label(s(&city["name"]), "vpn-city");
                name.set_hexpand(true);
                line.append(&name);
                if city["active"] == true {
                    line.append(&label("Current", "network-muted"));
                }
                b.set_child(Some(&line));
                cities.append(&b);
            }
            card.append(&cities);
        }
        self.body.append(&card);
        self.body.append(&label("Quick locations", "vpn-city"));
        let countries = gtk::Grid::new();
        countries.set_column_homogeneous(true);
        countries.set_column_spacing(8);
        countries.set_row_spacing(8);
        for (i, country) in arr(&info["countries"]).iter().take(30).enumerate() {
            let b = self.button(
                "",
                s(&country["name"]),
                context,
                "country",
                country["id"].clone(),
                info["locationsAvailable"] != false,
            );
            b.remove_css_class("panel-control");
            b.add_css_class("vpn-flag");

            b.set_hexpand(true);
            b.set_child(Some(&flag(s(&country["id"]), 64)));
            countries.attach(&b, i as i32 % 5, i as i32 / 5, 1, 1);
        }
        // Always reserve five slots, as in the original quick-location row.
        for i in arr(&info["countries"]).len()..5 {
            let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            spacer.set_hexpand(true);
            countries.attach(&spacer, i as i32, 0, 1, 1);
        }
        self.body.append(&countries);
        if info["locationsAvailable"] == false {
            self.body
                .append(&label("Locations are temporarily unavailable.", "vpn-city"));
        }
    }
    fn tailscale(self: &Rc<Self>, info: &Value, context: &Value) {
        let running = info["connection"] == "Running";
        let card = column(10);
        card.add_css_class("network-connection");
        let top = row();
        let names = column(4);
        names.set_hexpand(true);
        names.append(&label(
            info["device"].as_str().unwrap_or("This device"),
            "network-title",
        ));
        names.append(&label(
            match s(&info["connection"]) {
                "Running" => "Connected",
                "Stopped" => "Disconnected",
                "NeedsLogin" => "Sign-in required",
                _ => "Unavailable",
            },
            "network-muted",
        ));
        top.append(&names);
        let power = self.button(
            "power_settings_new",
            "Toggle Tailscale",
            context,
            "toggle",
            Value::Null,
            !s(&info["connection"]).is_empty(),
        );
        if running {
            power.add_css_class("network-accent");
        }
        top.append(&power);
        card.append(&top);
        let address = row();
        let text = label(
            info["address"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("No address assigned"),
            "vpn-city",
        );
        text.set_hexpand(true);
        text.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        address.append(&text);
        address.append(&self.button(
            "content_copy",
            "Copy this device address",
            context,
            "copy",
            json!("self"),
            !s(&info["address"]).is_empty(),
        ));
        card.append(&address);
        self.body.append(&card);
        let peers = column(6);
        for peer in arr(&info["peers"]).iter().take(32) {
            let line = row();
            line.add_css_class("vpn-peer");
            let dot = label(
                "●",
                if peer["online"] == true {
                    "vpn-online"
                } else {
                    "network-muted"
                },
            );
            line.append(&dot);
            let names = column(4);
            names.set_hexpand(true);
            names.append(&label(s(&peer["name"]), "network-label"));
            names.append(&label(
                &format!(
                    "{}{}",
                    if peer["online"] == true {
                        "Online"
                    } else {
                        "Offline"
                    },
                    peer["address"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(|s| format!(" · {s}"))
                        .unwrap_or_default()
                ),
                "network-muted",
            ));
            line.append(&names);
            line.append(&self.button(
                "network_ping",
                "Check reachability",
                context,
                "ping",
                peer["id"].clone(),
                running && peer["online"] == true,
            ));
            line.append(&self.button(
                "content_copy",
                "Copy device address",
                context,
                "copy",
                peer["id"].clone(),
                !s(&peer["address"]).is_empty(),
            ));
            peers.append(&line);
        }
        if arr(&info["peers"]).is_empty() {
            peers.append(&label("No other devices to show", "vpn-city"));
        }
        self.body.append(&peers);
        self.body.append(&divider());
        let footer = row();
        footer.append(&icon("devices"));
        let counts = label(
            &format!(
                "Devices · {} / {} online",
                info["online"].as_u64().unwrap_or(0),
                info["total"].as_u64().unwrap_or(0)
            ),
            "network-muted",
        );
        counts.set_hexpand(true);
        footer.append(&counts);
        footer.append(&self.button(
            "refresh",
            "Refresh devices",
            context,
            "refresh",
            Value::Null,
            true,
        ));
        footer.append(&self.button(
            "open_in_new",
            "Manage devices",
            context,
            "admin",
            Value::Null,
            true,
        ));
        self.body.append(&footer);
    }
}

#[cfg(test)]
pub fn exercise_fixture(card: &Rc<Card>, receive: &async_channel::Receiver<Event>) {
    fn find(widget: &gtk::Widget, name: &str) -> Option<gtk::Button> {
        if widget.widget_name() == name {
            if let Ok(b) = widget.clone().downcast::<gtk::Button>() {
                return Some(b);
            }
        }
        let mut child = widget.first_child();
        while let Some(w) = child {
            if let Some(b) = find(&w, name) {
                return Some(b);
            }
            child = w.next_sibling();
        }
        None
    }
    let click = |name: &str| {
        let b = find(card.body.upcast_ref(), name).expect(name);
        assert!(b.is_sensitive(), "{name}");
        b.emit_clicked();
    };
    assert!(card.body.measure(gtk::Orientation::Horizontal, -1).0 <= 388);
    assert!(
        card.body.measure(gtk::Orientation::Vertical, 388).1 < 650,
        "flag natural size inflated the combined card"
    );
    let flag = find(card.body.upcast_ref(), "Japan").unwrap();
    assert!(
        flag.width() < 80,
        "three country flags stretched to fill the row"
    );
    click("mullvad-cities");
    assert!(card.expanded.get());
    click("Osaka");
    assert!(!card.expanded.get());
    let action = loop {
        if let Event::ConnectivityAction(v) = receive.try_recv().unwrap() {
            break v;
        }
    };
    assert_eq!(action["module"], "vpn");
    assert_eq!(action["action"], "city");
    assert_eq!(action["selection"], json!({"country":"jp","city":"osa"}));
    card.acknowledge(&json!({"requestId":999,"busy":false}));
    assert!(card.pending.get().is_some());
    card.acknowledge(&json!({"requestId":action["requestId"],"busy":false,"error":true,"message":"Permission denied"}));
    assert!(card.pending.get().is_none());
    assert_eq!(*card.feedback.borrow(), "Permission denied");
    for (name, module, action_name) in [
        ("Japan", "vpn", "country"),
        ("Disconnect Mullvad", "vpn", "disconnect"),
        ("Toggle Tailscale", "tailnet", "toggle"),
        ("Copy this device address", "tailnet", "copy"),
        ("Check reachability", "tailnet", "ping"),
        ("Refresh devices", "tailnet", "refresh"),
        ("Manage devices", "tailnet", "admin"),
    ] {
        click(name);
        let action = loop {
            if let Event::ConnectivityAction(v) = receive.try_recv().unwrap() {
                break v;
            }
        };
        assert_eq!(action["module"], module);
        assert_eq!(action["action"], action_name);
        card.acknowledge(&json!({"requestId":action["requestId"],"busy":false,"message":"Done"}));
    }
    let mut empty = card.data.borrow().clone();
    empty["sections"][0]["row"]["panel"] = json!({"locationsAvailable":false});
    empty["sections"][1]["row"]["panel"] = json!({});
    card.update(empty);
    assert!(!find(card.body.upcast_ref(), "Connect Mullvad")
        .unwrap()
        .is_sensitive());
    assert!(!find(card.body.upcast_ref(), "Toggle Tailscale")
        .unwrap()
        .is_sensitive());
}
