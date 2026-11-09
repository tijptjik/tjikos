//! GTK presentation of normalized network state. No platform commands or paths.
use crate::{
    network::{AccessPoint, Action, Adapter, Counters, Kind, Snapshot, Traffic},
    network_controller::{self, Session, Update},
    panel_view::Content,
    Event,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};
use tjik_gtk::gtk::{self, glib, prelude::*};
fn label(text: &str, class: &str) -> gtk::Label {
    let widget = gtk::Label::new(Some(text));
    widget.set_xalign(0.);
    widget.add_css_class(class);
    widget
}
fn accent_symbol(text: &str) -> gtk::Label {
    let l = symbol(text);
    l.add_css_class("network-accent-symbol");
    l
}
fn symbol(text: &str) -> gtk::Label {
    crate::panel_view::control_symbol(text)
}
fn button(icon: &str, accessible: &str) -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class("panel-control");
    let glyph = symbol(icon);
    b.set_child(Some(&glyph));
    b.update_property(&[gtk::accessible::Property::Label(accessible)]);
    b
}
fn clear(body: &gtk::Box) {
    while let Some(child) = body.first_child() {
        body.remove(&child);
    }
}
fn fallback<'a>(value: &'a str, otherwise: &'a str) -> &'a str {
    if value.is_empty() {
        otherwise
    } else {
        value
    }
}
fn bytes(value: Option<f64>) -> String {
    let Some(mut value) = value.filter(|v| v.is_finite()) else {
        return "—".into();
    };
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut unit = 0;
    while value >= 1024. && unit < 4 {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", units[unit])
    } else {
        format!("{value:.1} {}", units[unit])
    }
}
struct TrafficLabels {
    interface: String,
    total: gtk::Label,
    rx: gtk::Label,
    tx: gtk::Label,
    detail: gtk::Label,
}
pub struct Card {
    pub body: gtk::Box,
    connections: gtk::Box,
    networks: gtk::Box,
    ethernet: gtk::Box,
    heading: gtk::Label,
    message: gtk::Label,
    scan: gtk::Button,
    radio: gtk::Button,
    list: gtk::ScrolledWindow,
    form: gtk::Box,
    selected_name: gtk::Label,
    password: gtk::PasswordEntry,
    connect: gtk::Button,
    state: RefCell<Option<Snapshot>>,
    selected: RefCell<Option<AccessPoint>>,
    busy: Cell<bool>,
    pending: Cell<bool>,
    controls: RefCell<Vec<gtk::Button>>,
    traffic: RefCell<Vec<TrafficLabels>>,
    session: RefCell<Option<Session>>,
    epoch: Cell<u64>,
    adapter: Arc<dyn Adapter>,
    send: async_channel::Sender<Event>,
}
impl Card {
    pub fn new(adapter: Arc<dyn Adapter>, send: async_channel::Sender<Event>) -> Rc<Self> {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        body.add_css_class("network-body");
        let connections = gtk::Box::new(gtk::Orientation::Vertical, 12);
        body.append(&connections);
        let wifi = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.set_margin_top(4);
        let heading = label("Wi-Fi networks", "network-label");
        heading.set_hexpand(true);
        bar.append(&heading);
        let scan = button("refresh", "Scan for networks");
        let radio = button("power_settings_new", "Toggle Wi-Fi");
        bar.append(&scan);
        bar.append(&radio);
        wifi.append(&bar);
        let message = label("Loading network status…", "network-muted");
        message.set_wrap(true);
        wifi.append(&message);
        let networks = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let list = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .propagate_natural_height(true)
            .max_content_height(192)
            .child(&networks)
            .build();
        wifi.append(&list);
        body.append(&wifi);
        let form = gtk::Box::new(gtk::Orientation::Vertical, 8);
        form.add_css_class("network-form");
        form.set_visible(false);
        let form_top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let selected_name = label("", "network-selected");
        selected_name.set_hexpand(true);
        selected_name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let cancel = button("close", "Cancel connection");
        let connect = button("check", "Connect");
        connect.add_css_class("network-accent");
        form_top.append(&selected_name);
        form_top.append(&cancel);

        form.append(&form_top);
        let password = gtk::PasswordEntry::new();
        password.set_show_peek_icon(false);
        password.set_placeholder_text(Some("Password · blank uses saved credentials"));
        password.update_property(&[gtk::accessible::Property::Label(
            "Wi-Fi password; blank uses saved credentials",
        )]);
        password.add_css_class("network-password");
        let field = gtk::Overlay::new();
        field.set_child(Some(&password));
        connect.add_css_class("network-password-submit");
        connect.set_halign(gtk::Align::End);
        connect.set_valign(gtk::Align::Center);
        connect.set_margin_end(5);
        connect.set_visible(false);
        field.add_overlay(&connect);
        form.append(&field);
        wifi.append(&form);
        let ethernet = gtk::Box::new(gtk::Orientation::Vertical, 10);
        body.append(&ethernet);
        let card = Rc::new(Self {
            body,
            connections,
            networks,
            ethernet,
            heading,
            message,
            scan,
            radio,
            list,
            form,
            selected_name,
            password,
            connect,
            state: RefCell::new(None),
            selected: RefCell::new(None),
            busy: Cell::new(true),
            pending: Cell::new(false),
            controls: RefCell::new(vec![]),
            traffic: RefCell::new(vec![]),
            session: RefCell::new(None),
            epoch: Cell::new(0),
            adapter,
            send,
        });
        let weak = Rc::downgrade(&card);
        card.scan.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.run(Action::Scan);
            }
        });
        let weak = Rc::downgrade(&card);
        card.radio.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                let enabled = !c.state.borrow().as_ref().is_some_and(|s| s.enabled);
                c.run(Action::Radio { enabled });
            }
        });
        let weak = Rc::downgrade(&card);
        cancel.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.select(None);
            }
        });
        let weak = Rc::downgrade(&card);
        card.connect.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.connect_selected();
            }
        });
        let submit = card.connect.downgrade();
        card.password.connect_changed(move |entry| {
            if let Some(submit) = submit.upgrade() {
                submit.set_visible(!entry.text().is_empty());
            }
        });
        let weak = Rc::downgrade(&card);
        card.password.connect_activate(move |_| {
            if let Some(c) = weak.upgrade() {
                c.connect_selected();
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
        card.sensitivity();
        card
    }
    pub fn content(&self) -> Content {
        Content {
            title: "Networking".into(),
            body: self.body.clone().upcast(),
            footer: None,
            preferred_width: 420.,
            needs_keyboard: true,
        }
    }
    pub fn diagnostics(&self) -> serde_json::Value {
        serde_json::json!({"adapter":self.adapter.name(),"active":self.session.borrow().is_some(),"busy":self.busy.get()})
    }
    fn resized(&self) {
        let _ = self.send.try_send(Event::PanelCommand("resize".into()));
    }
    fn start(self: &Rc<Self>) {
        if self.session.borrow().is_some() {
            return;
        }
        self.busy.set(true);
        self.sensitivity();
        let epoch = self.epoch.get();
        let (session, updates) = network_controller::start(self.adapter.clone());
        *self.session.borrow_mut() = Some(session);
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(update) = updates.recv().await {
                let Some(card) = weak.upgrade() else {
                    break;
                };
                if card.epoch.get() != epoch {
                    break;
                }
                match update {
                    Update::Snapshot(result, completed_action) => {
                        if completed_action {
                            card.pending.set(false);
                        }
                        card.busy.set(card.pending.get());
                        match result {
                            Ok(snapshot) => {
                                card.message.set_visible(false);
                                if card.state.borrow().as_ref() != Some(&snapshot) {
                                    card.render(snapshot);
                                }
                            }
                            Err(error) => {
                                card.message.set_text(&error.to_string());
                                card.message.set_visible(true);
                            }
                        }
                        card.sensitivity();
                        card.resized();
                    }
                    Update::Traffic(traffic) => card.update_traffic(&traffic),
                }
            }
        });
    }
    fn stop(&self) {
        self.epoch.set(self.epoch.get().wrapping_add(1));
        self.session.borrow_mut().take();
        self.pending.set(false);
        self.password.set_text("");
        self.selected.borrow_mut().take();
        self.form.set_visible(false);
        self.list.set_visible(true);
    }
    fn sensitivity(&self) {
        let ready = !self.busy.get();
        let capabilities = self.adapter.capabilities();
        let state = self.state.borrow();
        let wifi_present = state
            .as_ref()
            .is_some_and(|s| s.devices.iter().any(|d| d.kind == Kind::Wifi));
        self.scan.set_sensitive(
            ready && capabilities.wifi && wifi_present && state.as_ref().is_some_and(|s| s.enabled),
        );
        self.radio
            .set_sensitive(ready && capabilities.wifi && wifi_present);
        self.connect.set_sensitive(ready && capabilities.wifi);
        self.password.set_sensitive(ready);
        for b in self.controls.borrow().iter() {
            b.set_sensitive(ready);
        }
    }
    fn run(&self, action: Action) {
        if self.busy.get() {
            return;
        }
        let result = self
            .session
            .borrow()
            .as_ref()
            .ok_or("Network panel is not active")
            .and_then(|s| s.send(action));
        match result {
            Ok(()) => {
                self.busy.set(true);
                self.pending.set(true);
                self.message.set_text("Working…");
            }
            Err(error) => self.message.set_text(error),
        }
        self.message.set_visible(true);
        self.sensitivity();
        self.resized();
    }
    fn select(&self, ap: Option<AccessPoint>) {
        self.password.set_text("");
        self.form.set_visible(ap.is_some());
        self.list.set_visible(ap.is_none());
        if let Some(ap) = &ap {
            self.selected_name.set_text(&ap.ssid);
            self.password
                .set_visible(!ap.security.is_empty() && ap.security != "--");
        }
        let open = ap
            .as_ref()
            .is_some_and(|ap| ap.security.is_empty() || ap.security == "--");
        *self.selected.borrow_mut() = ap;
        if open {
            self.connect_selected();
            return;
        }
        if self.password.is_visible() && self.form.is_visible() {
            self.password.grab_focus();
        }
        self.resized();
    }
    fn connect_selected(&self) {
        if self.busy.get() {
            return;
        }
        let Some(ap) = self.selected.borrow().clone() else {
            return;
        };
        self.run(Action::Connect {
            interface: ap.interface,
            bssid: ap.bssid,
            password: self.password.text().to_string(),
        });
        self.select(None);
    }
    fn render(self: &Rc<Self>, snapshot: Snapshot) {
        clear(&self.connections);
        clear(&self.networks);
        clear(&self.ethernet);
        self.controls.borrow_mut().clear();
        self.traffic.borrow_mut().clear();
        if self.selected.borrow().as_ref().is_some_and(|ap| {
            !snapshot.enabled
                || !snapshot
                    .networks
                    .iter()
                    .any(|n| n.bssid == ap.bssid && n.interface == ap.interface && !n.active)
        }) {
            self.select(None);
        }
        for device in snapshot.devices.iter().filter(|d| d.connected()) {
            let section = gtk::Box::new(gtk::Orientation::Vertical, 10);
            section.add_css_class("network-connection");
            let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            top.append(&accent_symbol(if device.kind == Kind::Wifi {
                "wifi"
            } else {
                "lan"
            }));
            let ap = snapshot
                .networks
                .iter()
                .find(|n| n.active && n.interface == device.interface);
            let name = ap
                .map(|n| n.ssid.as_str())
                .unwrap_or(fallback(&device.connection, &device.interface));
            let names = gtk::Box::new(gtk::Orientation::Vertical, 2);
            names.set_hexpand(true);
            let title = label(name, "network-title");
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            names.append(&title);
            let connection_detail = label(
                &format!(
                    "Connected · {}{}",
                    device.interface,
                    ap.map(|n| format!(" · {}%", n.signal)).unwrap_or_default()
                ),
                "network-muted",
            );
            connection_detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
            names.append(&connection_detail);
            top.append(&names);
            let disconnect = button("link_off", &format!("Disconnect {name}"));
            let interface = device.interface.clone();
            let weak = Rc::downgrade(self);
            disconnect.connect_clicked(move |_| {
                if let Some(c) = weak.upgrade() {
                    c.run(Action::Disconnect {
                        interface: interface.clone(),
                    });
                }
            });
            self.controls.borrow_mut().push(disconnect.clone());
            top.append(&disconnect);
            section.append(&top);
            section.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            let traffic = gtk::Box::new(gtk::Orientation::Vertical, 8);
            let values = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            // Different values need different widths; never multiply the longest
            // rate by three and force the whole panel past its fixed shell.
            values.set_homogeneous(false);
            let mut labels = vec![];
            for (icon, name) in [
                ("swap_vert", "Transferred"),
                ("arrow_downward", "Download"),
                ("arrow_upward", "Upload"),
            ] {
                let group = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                group.append(&accent_symbol(icon));
                group.set_hexpand(true);
                let column = gtk::Box::new(gtk::Orientation::Vertical, 2);
                column.set_hexpand(true);
                let title = label(name, "network-muted");
                title.set_ellipsize(gtk::pango::EllipsizeMode::End);
                column.append(&title);
                let value = label("—", "network-title");
                value.set_ellipsize(gtk::pango::EllipsizeMode::End);
                column.append(&value);
                labels.push(value);
                group.append(&column);
                values.append(&group);
            }
            traffic.append(&values);
            let detail = label("", "network-muted");
            detail.set_xalign(0.5);
            detail.set_wrap(true);
            traffic.append(&detail);
            section.append(&traffic);
            self.traffic.borrow_mut().push(TrafficLabels {
                interface: device.interface.clone(),
                total: labels[0].clone(),
                rx: labels[1].clone(),
                tx: labels[2].clone(),
                detail,
            });
            section.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            let addresses = gtk::Grid::new();
            addresses.set_column_spacing(12);
            addresses.set_row_spacing(4);
            let dns = if snapshot.default_dns.is_empty() {
                &device.addresses.dns
            } else {
                &snapshot.default_dns
            };
            for (i, (name, value)) in [
                (
                    "Local IP",
                    fallback(&device.addresses.ip.join(", "), "No IPv4 address").to_owned(),
                ),
                (
                    "Gateway",
                    fallback(&device.addresses.gateway, "None").into(),
                ),
                (
                    "DHCP server",
                    fallback(&device.addresses.dhcp, "Not reported / static").into(),
                ),
                (
                    if snapshot.default_dns.is_empty() {
                        "Link DNS"
                    } else {
                        "Default DNS"
                    },
                    fallback(&dns.join(", "), "None").into(),
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let name = label(name, "network-address-name");
                name.set_width_request(96);
                name.set_valign(gtk::Align::Start);
                let value = label(&value, "network-label");
                value.set_selectable(true);
                value.set_wrap(true);
                value.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                value.set_hexpand(true);
                addresses.attach(&name, 0, i as i32, 1, 1);
                addresses.attach(&value, 1, i as i32, 1, 1);
            }
            section.append(&addresses);
            self.connections.append(&section);
        }
        self.connections
            .set_visible(self.connections.first_child().is_some());
        self.heading.set_text(if snapshot.enabled {
            "Wi-Fi networks"
        } else {
            "Wi-Fi is off"
        });
        if snapshot.enabled {
            self.radio.add_css_class("network-accent");
        } else {
            self.radio.remove_css_class("network-accent");
        }
        let mut count = 0;
        if snapshot.enabled {
            for ap in snapshot.networks.iter().filter(|n| !n.active).take(128) {
                let row = gtk::Button::new();
                row.add_css_class("network-row");
                row.update_property(&[gtk::accessible::Property::Label(&format!(
                    "Connect to {}",
                    ap.ssid
                ))]);
                let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let names = gtk::Box::new(gtk::Orientation::Vertical, 2);
                names.set_hexpand(true);
                let name = label(&ap.ssid, "network-label");
                name.set_ellipsize(gtk::pango::EllipsizeMode::End);
                names.append(&name);
                let detail = label(
                    &format!(
                        "{}% · {} · {}",
                        ap.signal,
                        fallback(&ap.security, "Open"),
                        ap.interface
                    ),
                    "network-muted",
                );
                detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
                names.append(&detail);
                line.append(&names);
                line.append(&symbol("chevron_right"));
                row.set_child(Some(&line));
                let ap = ap.clone();
                let weak = Rc::downgrade(self);
                row.connect_clicked(move |_| {
                    if let Some(c) = weak.upgrade() {
                        c.select(Some(ap.clone()));
                    }
                });
                self.controls.borrow_mut().push(row.clone());
                self.networks.append(&row);
                count += 1;
            }
        }
        if count == 0 {
            self.networks.append(&label(
                if !snapshot.devices.iter().any(|d| d.kind == Kind::Wifi) {
                    "No Wi-Fi adapter found."
                } else if !snapshot.enabled {
                    "Turn on Wi-Fi to find networks."
                } else if !snapshot.networks.is_empty() {
                    "No other networks nearby"
                } else {
                    "No networks found · try scanning"
                },
                "network-muted",
            ));
        }
        // The reference shows three complete two-line rows, then scrolls.
        let mut child = self.networks.first_child();
        let mut height = 0;
        for i in 0..count.min(3) {
            let Some(row) = child else {
                break;
            };
            height += row.measure(gtk::Orientation::Vertical, 388).1 + if i > 0 { 6 } else { 0 };
            child = row.next_sibling();
        }
        self.list.set_min_content_height(-1);
        self.list.set_max_content_height(height.max(24));
        self.list.set_min_content_height(height.max(24));
        for d in snapshot
            .devices
            .iter()
            .filter(|d| d.kind == Kind::Ethernet && !d.connected())
        {
            self.ethernet
                .append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.append(&symbol("lan"));
            let name = label(
                &format!("Ethernet · {} · {}", d.interface, d.state),
                "network-muted",
            );
            name.set_hexpand(true);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            row.append(&name);
            let connect = button("link", "Connect Ethernet");
            if !["unavailable", "unmanaged"].contains(&d.state.as_str())
                && self.adapter.capabilities().ethernet
            {
                let interface = d.interface.clone();
                let weak = Rc::downgrade(self);
                connect.connect_clicked(move |_| {
                    if let Some(c) = weak.upgrade() {
                        c.run(Action::Ethernet {
                            interface: interface.clone(),
                        });
                    }
                });
                self.controls.borrow_mut().push(connect.clone());
            } else {
                connect.set_sensitive(false);
            }
            row.append(&connect);
            self.ethernet.append(&row);
        }
        self.ethernet
            .set_visible(self.ethernet.first_child().is_some());
        *self.state.borrow_mut() = Some(snapshot);
        self.sensitivity();
        self.resized();
    }
    fn update_traffic(&self, traffic: &Traffic) {
        for row in self.traffic.borrow().iter() {
            let empty = Counters::default();
            let c = traffic.get(&row.interface).unwrap_or(&empty);
            row.total
                .set_text(&bytes(c.rx.zip(c.tx).map(|(a, b)| a + b)));
            for (label, rate) in [(&row.rx, c.rx_rate), (&row.tx, c.tx_rate)] {
                label.set_text(&format!(
                    "{}{}",
                    bytes(rate),
                    if rate.is_some() { "/s" } else { "" }
                ));
            }
            row.detail.set_text(&format!(
                "↓ {}   ↑ {} · since adapter reset",
                bytes(c.rx),
                bytes(c.tx)
            ));
        }
    }
}

#[cfg(test)]
pub fn wide_values_fixture(card: &Rc<Card>) {
    let traffic = std::collections::BTreeMap::from([(
        "wlan-demo".into(),
        Counters {
            rx: Some(121.3 * 1024f64.powi(3)),
            tx: Some(16.7 * 1024f64.powi(3)),
            rx_rate: Some(120.5 * 1024.),
            tx_rate: Some(73.3 * 1024.),
        },
    )]);
    card.update_traffic(&traffic);
    assert!(
        card.body.measure(gtk::Orientation::Horizontal, -1).0 <= 388,
        "live-sized traffic values widened the panel body: {}",
        card.body.measure(gtk::Orientation::Horizontal, -1).0
    );
}
#[cfg(test)]
pub fn assert_controls_fit(card: &Rc<Card>) {
    for button in card
        .controls
        .borrow()
        .iter()
        .chain([&card.scan, &card.radio])
    {
        let bounds = button.compute_bounds(&card.body).unwrap();
        assert!(
            bounds.x() >= 0. && bounds.x() + bounds.width() <= card.body.width() as f32 + 0.5,
            "network control escaped body: {bounds:?}; body width {}",
            card.body.width()
        );
    }
}

#[cfg(test)]
pub fn exercise_fixture(card: &Rc<Card>, fake: &Arc<crate::network::Fake>) {
    fn wait(mut done: impl FnMut() -> bool) {
        let context = glib::MainContext::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !done() && std::time::Instant::now() < deadline {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(done(), "network UI did not settle");
    }
    wait(|| !card.busy.get());
    assert!(
        card.list.height() <= 186,
        "more than three Wi-Fi rows visible"
    );
    let row = card
        .networks
        .first_child()
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap();
    row.emit_clicked();
    assert!(card.form.is_visible());
    assert!(!card.list.is_visible());
    card.select(None);
    assert!(card.list.is_visible());
    assert!(!card.form.is_visible());
    row.emit_clicked();
    assert!(!card.list.is_visible());
    assert!(!card.connect.is_visible());
    card.password.set_text("synthetic-secret");
    assert!(card.connect.is_visible());
    card.password.set_text("");
    assert!(!card.connect.is_visible());
    card.password.set_text("synthetic-secret");
    card.connect.emit_clicked();
    assert!(card.password.text().is_empty());
    assert!(!card.form.is_visible());
    assert!(card.list.is_visible());
    wait(|| !card.busy.get());
    assert!(fake
        .commands
        .lock()
        .unwrap()
        .contains(&"connect".to_string()));
    *fake.failure.lock().unwrap() = Some(crate::network::Error::PermissionDenied);
    card.scan.emit_clicked();
    wait(|| !card.busy.get());
    assert!(card.message.is_visible());
    assert!(card.message.text().contains("permission denied"));
    *fake.failure.lock().unwrap() = None;
    card.scan.emit_clicked();
    wait(|| !card.busy.get());
    assert!(!card.message.is_visible());
    card.run(Action::Ethernet {
        interface: "eth-demo".into(),
    });
    wait(|| !card.busy.get());
    assert!(card
        .state
        .borrow()
        .as_ref()
        .unwrap()
        .devices
        .iter()
        .any(|d| d.kind == Kind::Ethernet && d.connected()));
    card.radio.emit_clicked();
    wait(|| !card.busy.get());
    assert_eq!(card.heading.text(), "Wi-Fi is off");
    assert!(!card.scan.is_sensitive());
    card.radio.emit_clicked();
    wait(|| !card.busy.get());
    let ap = card.state.borrow().as_ref().unwrap().networks[0].clone();
    card.select(Some(ap));
    card.password.set_text("must-clear-on-removal");
    fake.state.lock().unwrap().networks.clear();
    card.run(Action::Scan);
    wait(|| !card.busy.get());
    assert!(!card.form.is_visible());
    assert!(card.password.text().is_empty());
    card.stop();
    assert!(card.session.borrow().is_none());
}
