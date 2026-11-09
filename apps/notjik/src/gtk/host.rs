//! Native popup host. GTK owns surfaces; Controller owns notification semantics.
use super::{desktop_bus::Bus, executor::Executor, model::*, primitives, Card};
use crate::{
    backend_channel,
    controller::{Controller, Effect},
    ipc,
};
use anyhow::{Context, Result};
use gtk::{gdk, gio, glib, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
struct Surface {
    window: gtk::ApplicationWindow,
    list: gtk::Box,
    cards: HashMap<String, Rc<Card>>,
    snapshots: HashMap<String, Value>,
    states: HashMap<String, String>,
    keyboard_selected: bool,
}
struct Detached {
    monitor: String,
    id: String,
    x: i32,
    y: i32,
}
struct Host {
    root: PathBuf,
    controller: Rc<RefCell<Controller>>,
    bus: Option<Bus>,
    socket: Option<ipc::Server>,
    requests: mpsc::Receiver<ipc::Request>,
    backend: Option<backend_channel::Client>,
    pending: VecDeque<Value>,
    events: mpsc::Receiver<CardEvent>,
    event_tx: mpsc::Sender<CardEvent>,
    executor: Executor,
    windows: HashMap<String, Surface>,
    detached: HashMap<String, Detached>,
    feedback: HashMap<String, String>,
    busy: HashSet<String>,
    provider: Option<gtk::CssProvider>,
    surface_provider: Option<gtk::CssProvider>,
    style: Value,
    last: Instant,
    monitor_tick: Instant,
    desktop: mpsc::Receiver<Value>,
    app: gtk::Application,
    hold: Option<gio::ApplicationHoldGuard>,
    stopped: bool,
}
impl Host {
    fn new(app: &gtk::Application, root: PathBuf) -> Result<Rc<RefCell<Self>>> {
        let config = crate::config::load(&root)?;
        anyhow::ensure!(
            gtk4_layer_shell::is_supported() || std::env::var_os("TJIK_GTK_WINDOWED").is_some(),
            "GTK layer-shell is unavailable"
        );
        let controller = Rc::new(RefCell::new(Controller::new(
            config,
            format!("gtk-{}-{}", now(), std::process::id()),
        )));
        let bus = Bus::acquire(controller.clone())?;
        let (socket, requests) = ipc::Server::bind()?;
        let (client, mut transport) = backend_channel::channel();
        let backend_root = root.clone();
        std::thread::spawn(move || {
            if let Err(e) = crate::backend::run(&backend_root, &mut transport) {
                eprintln!("notjik backend: {e}");
            }
        });
        let (event_tx, events) = mpsc::channel();
        let (desktop_tx, desktop) = mpsc::sync_channel(2);
        std::thread::spawn(move || loop {
            let v = crate::util::run(
                std::process::Command::new("hyprctl").args(["-j", "activewindow"]),
                Duration::from_secs(2),
            )
            .ok()
            .and_then(|v| serde_json::from_slice::<Value>(&v).ok())
            .unwrap_or(json!({}));
            if desktop_tx.send(v).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        });
        let host = Rc::new(RefCell::new(Self {
            root,
            controller,
            bus: Some(bus),
            socket: Some(socket),
            requests,
            backend: Some(client),
            pending: VecDeque::new(),
            events,
            event_tx,
            executor: Executor::new(),
            windows: HashMap::new(),
            detached: HashMap::new(),
            feedback: HashMap::new(),
            busy: HashSet::new(),
            provider: None,
            surface_provider: None,
            style: Value::Null,
            last: Instant::now(),
            monitor_tick: Instant::now() - Duration::from_secs(2),
            desktop,
            app: app.clone(),
            hold: Some(app.hold()),
            stopped: false,
        }));
        let weak = Rc::downgrade(&host);
        app.connect_shutdown(move |_| {
            if let Some(h) = weak.upgrade() {
                h.borrow_mut().stop();
            }
        });
        let tick = host.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let mut h = tick.borrow_mut();
            if h.stopped {
                return glib::ControlFlow::Break;
            }
            h.tick();
            glib::ControlFlow::Continue
        });
        Ok(host)
    }
    fn stop(&mut self) {
        self.stopped = true;
        self.executor.stop_audio();
        self.executor.discard();
        self.bus.take();
        self.socket.take();
        self.backend.take();
        self.hold.take();
    }
    fn tick(&mut self) {
        if self
            .bus
            .as_ref()
            .is_some_and(|bus| bus.connection.is_closed())
        {
            self.controller.borrow_mut().disconnected();
            self.stop();
            for surface in self.windows.values() {
                surface.window.set_visible(false);
            }
            self.app.quit();
            return;
        }
        let elapsed = self.last.elapsed().as_millis() as u64;
        self.last = Instant::now();
        if let Some(client) = &self.backend {
            loop {
                match client.try_recv() {
                    Ok(event) => self.controller.borrow_mut().update(event),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.controller.borrow_mut().disconnected();
                        break;
                    }
                }
            }
        }
        while let Ok(desktop) = self.desktop.try_recv() {
            self.controller.borrow_mut().focused_app =
                desktop["class"].as_str().unwrap_or("").into();
        }
        if self.monitor_tick.elapsed() >= Duration::from_secs(1) {
            self.monitors();
            self.monitor_tick = Instant::now();
        }
        let audio_id = self.executor.audio_id();
        if self.controller.borrow().concealed() || self.controller.borrow().locked() {
            self.executor.stop_audio();
            if self
                .executor
                .capture
                .as_ref()
                .is_some_and(|c| ["starting", "recording"].contains(&c.state.phase.as_str()))
            {
                let _ = self.executor.stop_recording(false);
            }
        }
        if audio_id
            .as_ref()
            .is_some_and(|id| self.controller.borrow().find(id).is_none())
        {
            self.executor.stop_audio();
        }
        self.executor.audio_tick();
        if self.executor.audio_id().is_none() {
            if let Some(id) = audio_id {
                if !self.busy.contains(&id)
                    && self.executor.capture.as_ref().is_none_or(|r| r.id != id)
                {
                    self.controller.borrow_mut().hold(&id, false);
                }
            }
        }

        while let Ok(done) = self.executor.rx.try_recv() {
            if done.kind == "capture" || done.kind == "capture-exited" {
                self.executor.capture_event(&done);
                let should_send = self
                    .executor
                    .capture
                    .as_ref()
                    .is_some_and(|c| c.state.phase == "ready" && c.send_when_ready);
                if should_send
                    && self
                        .controller
                        .borrow()
                        .find(&done.id)
                        .is_some_and(|n| self.controller.borrow().allowed(n))
                {
                    let _ = self.executor.send_voice();
                }
                continue;
            }
            self.busy.remove(&done.id);
            if done.kind == "voiceReply" {
                if done.result["ok"] == true {
                    if let Some(c) = &mut self.executor.capture {
                        c.state.phase = "sent".into();
                    }
                    self.executor.discard();
                } else if let Some(c) = &mut self.executor.capture {
                    c.state.phase = "ready".into();
                    c.state.uncertain = done.result["uncertain"] == true;
                    c.state.error = text(&done.result["error"]);
                }
            }
            if done.result["ok"] == true {
                if ["reply", "open", "task"].contains(&done.kind.as_str()) {
                    self.controller.borrow_mut().close(&done.id, 2);
                } else {
                    self.feedback
                        .insert(done.id.clone(), fallback(&done.result["message"], "Done"));
                }
            } else {
                self.feedback.insert(
                    done.id.clone(),
                    fallback(&done.result["error"], "Action failed"),
                );
            }
            self.controller.borrow_mut().hold(&done.id, false);
        }
        while let Ok(event) = self.events.try_recv() {
            let id = event.id.clone();
            if let Err(e) = self.event(event) {
                self.feedback.insert(id, e.to_string());
            }
        }
        while let Ok(request) = self.requests.try_recv() {
            let result = self
                .controller
                .borrow_mut()
                .handle(request.value, now())
                .unwrap_or_else(|e| json!({"ok":false,"error":e.to_string()}));
            let _ = request.reply.send(result);
        }
        self.controller.borrow_mut().tick(elapsed);
        for effect in std::mem::take(&mut self.controller.borrow_mut().effects) {
            match effect {
                Effect::Backend(v) => self.pending.push_back(v),
                Effect::Closed(id, reason) => {
                    if let Some(bus) = &self.bus {
                        bus.closed(id, reason);
                    }
                }
                Effect::Action(id, key) => {
                    if let Some(bus) = &self.bus {
                        bus.action(id, &key);
                    }
                }
                Effect::Copy(s) => {
                    if let Some(display) = gdk::Display::default() {
                        display.clipboard().set_text(&s);
                    }
                }
                Effect::Command(args) => {
                    self.executor.command(String::new(), "command".into(), args)
                }
            }
        }
        if let Some(client) = &self.backend {
            while let Some(v) = self.pending.front() {
                if client.submit(v.clone()).is_err() {
                    break;
                }
                self.pending.pop_front();
            }
        }
        if let Some(c) = &self.executor.capture {
            if self.controller.borrow().find(&c.id).is_none()
                || self
                    .controller
                    .borrow()
                    .find(&c.id)
                    .is_some_and(|n| self.controller.borrow().blocked(n))
            {
                self.executor.discard();
            }
        }
        let active = self
            .controller
            .borrow()
            .notices
            .iter()
            .map(|n| text(&n["id"]))
            .collect::<HashSet<_>>();
        for key in self
            .detached
            .iter()
            .filter(|(_, d)| !active.contains(&d.id))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>()
        {
            self.detached.remove(&key);
            if let Some(surface) = self.windows.remove(&key) {
                surface.window.close();
            }
        }
        self.feedback.retain(|id, _| active.contains(id));
        self.busy.retain(|id| active.contains(id));
        self.render();
    }
    fn event(&mut self, event: CardEvent) -> Result<()> {
        let id = event.id;
        let mut c = self.controller.borrow_mut();
        match &event.intent {
            Intent::Hover(on) => {
                c.hover(&id, *on);
                return Ok(());
            }
            Intent::Hold(on) => {
                c.hold(&id, *on);
                return Ok(());
            }
            _ => {}
        }
        if id.is_empty() && matches!(event.intent, Intent::Select) {
            c.selected.clear();
            return Ok(());
        }
        let n = c
            .find(&id)
            .cloned()
            .context("Notification no longer active")?;
        anyhow::ensure!(c.allowed(&n), "Actions unavailable");
        match event.intent {
            Intent::Dismiss => {
                let shown = c.visible().iter().map(|n| text(&n["id"])).collect();
                c.dismiss(&id, &shown);
            }
            Intent::Invoke(index) => c.invoke(&id, index)?,
            Intent::Select => {
                c.selected = id;
            }
            Intent::OpenGroup => {
                let members = c
                    .visible()
                    .iter()
                    .find(|v| v["id"] == id)
                    .map(|v| v["memberIds"].as_array().cloned().unwrap_or_default())
                    .unwrap_or_default();
                let i = members.iter().position(|v| v == &id).unwrap_or(0);
                if !members.is_empty() {
                    c.selected = text(&members[(i + 1) % members.len()]);
                }
            }
            Intent::Telegram {
                action,
                text,
                quote,
                chat_id,
                message_id,
            } => {
                anyhow::ensure!(!self.busy.contains(&id), "Action already running");
                self.busy.insert(id.clone());
                c.hold(&id, true);
                self.executor.action(id,action.clone(),json!({"action":action,"text":text,"quote":quote,"chatId":chat_id,"messageId":message_id}),n["data"]["demo"]==true);
            }
            Intent::Task { action, until } => {
                anyhow::ensure!(n["data"]["demo"] != true, "Synthetic task; nothing changed");
                let mut args = vec![
                    "tasktjik".into(),
                    action,
                    text(&n["data"]["taskId"]),
                    text(&n["data"]["period"]),
                ];
                if let Some(until) = until {
                    args.push(until);
                }
                self.busy.insert(id.clone());
                c.hold(&id, true);
                self.executor.command(id, "task".into(), args);
            }
            Intent::PlaybackToggle => {
                self.executor.play(&id, &n, true)?;
                c.hold(
                    &id,
                    self.executor.playback.get(&id).is_some_and(|p| p.playing),
                );
            }
            Intent::Seek(ms) => {
                let playing = self.executor.playback.get(&id).is_some_and(|p| p.playing);
                self.executor.stop_audio();
                self.executor
                    .playback
                    .entry(id.clone())
                    .or_default()
                    .position_ms = ms;
                if playing {
                    self.executor.play(&id, &n, false)?;
                }
            }
            Intent::PlaybackRate(rate) => {
                anyhow::ensure!([1., 1.5, 2.].contains(&rate), "Invalid playback rate");
                let playing = self.executor.playback.get(&id).is_some_and(|p| p.playing);
                self.executor.stop_audio();
                self.executor.playback.entry(id.clone()).or_default().rate = rate;
                if playing {
                    self.executor.play(&id, &n, false)?;
                }
            }
            Intent::Record => {
                self.executor.record(&self.root, id.clone(), &n)?;
                c.hold(&id, true);
            }
            Intent::RecordDiscard => {
                self.executor.discard();
                c.hold(&id, false);
            }
            Intent::RecordSend => self.executor.send_voice()?,
            Intent::Drag { dx, dy, finished } => {
                if !finished || dx.abs() + dy.abs() < 10. {
                    return Ok(());
                }
                let key = format!("floating:{id}");
                if let Some(d) = self.detached.get_mut(&key) {
                    d.x = (d.x + dx as i32).max(0);
                    d.y = (d.y + dy as i32).max(0);
                } else if let Some((name, surface)) =
                    self.windows.iter().find(|(_, w)| w.cards.contains_key(&id))
                {
                    if let Some(monitor) = surface.window.monitor() {
                        let origin = surface.cards[&id]
                            .widget
                            .compute_point(&surface.window, &gtk::graphene::Point::new(0., 0.))
                            .unwrap_or(gtk::graphene::Point::new(0., 0.));
                        let bounds = monitor.geometry();
                        let x = (surface.window.margin(Edge::Left) + origin.x() as i32 + dx as i32)
                            .clamp(0, (bounds.width() - surface.window.width()).max(0));
                        let y = (surface.window.margin(Edge::Top) + origin.y() as i32 + dy as i32)
                            .clamp(
                                0,
                                (bounds.height() - surface.cards[&id].widget.height()).max(0),
                            );
                        self.detached.insert(
                            key.clone(),
                            Detached {
                                monitor: name.clone(),
                                id: id.clone(),
                                x,
                                y,
                            },
                        );
                        self.windows.insert(
                            key,
                            popup(
                                &self.app,
                                &monitor,
                                &c.config,
                                &self.event_tx,
                                self.controller.clone(),
                            ),
                        );
                        c.floating.insert(id);
                    }
                }
            }
            Intent::Hover(_) | Intent::Hold(_) => {}
        }
        Ok(())
    }
    fn monitors(&mut self) {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let config = self.controller.borrow().config.clone();
        let mut monitors = vec![];
        let mut selected = HashSet::new();
        for (i, monitor) in display
            .monitors()
            .iter::<gdk::Monitor>()
            .flatten()
            .enumerate()
        {
            let name = monitor
                .connector()
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("monitor-{i}"));
            let p = placement(&config, &name, i);
            let selector = &config["placement"]["monitors"];
            let primary = config["placement"]["primaryMonitor"]
                .as_str()
                .filter(|s| !s.is_empty());
            let chosen = if selector == "primary" {
                primary.map(|s| s == name).unwrap_or(i == 0)
            } else {
                selector == "all"
                    || selector
                        .as_array()
                        .is_some_and(|a| a.contains(&json!(i + 1)) || a.contains(&json!(name)))
            } && p["enabled"] != false;
            let geometry = monitor.geometry();
            monitors.push(json!({"index":i+1,"name":name,"width":geometry.width(),"height":geometry.height(),"scale":monitor.scale_factor(),"selected":chosen}));
            if !chosen {
                continue;
            }
            selected.insert(name.clone());
            if !self.windows.contains_key(&name) {
                self.windows.insert(
                    name,
                    popup(
                        &self.app,
                        &monitor,
                        &config,
                        &self.event_tx,
                        self.controller.clone(),
                    ),
                );
            }
        }
        let removed = self
            .windows
            .keys()
            .filter(|name| {
                !selected.contains(*name)
                    && !self
                        .detached
                        .get(*name)
                        .is_some_and(|d| selected.contains(&d.monitor))
            })
            .cloned()
            .collect::<Vec<_>>();
        for name in removed {
            if let Some(surface) = self.windows.remove(&name) {
                surface.window.close();
            }
            if let Some(d) = self.detached.remove(&name) {
                self.controller.borrow_mut().floating.remove(&d.id);
            }
        }
        self.controller.borrow_mut().monitors = monitors
            .into_iter()
            .filter(|v| v["selected"] == true)
            .collect();
    }
    fn render(&mut self) {
        let c = self.controller.borrow();
        let style = &c.config["style"];
        if self.style != *style {
            if let (Some(display), Some(provider)) = (gdk::Display::default(), self.provider.take())
            {
                gtk::style_context_remove_provider_for_display(&display, &provider);
            }
            let provider = primitives::apply_theme(style);
            if let (Some(display), Some(provider)) =
                (gdk::Display::default(), self.surface_provider.take())
            {
                gtk::style_context_remove_provider_for_display(&display, &provider);
            }
            let extra = gtk::CssProvider::new();
            extra.load_from_string("window.notjik-popup, window.notjik-popup > scrolledwindow {background:transparent;}");
            if let Some(display) = gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &extra,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
                );
            }
            self.surface_provider = Some(extra);
            self.provider = Some(provider);
            self.style = style.clone();
        }
        let stack = c.visible();
        for (name, surface) in &mut self.windows {
            let detached = self.detached.get(name);
            let visible = if let Some(d) = detached {
                if c.concealed() {
                    vec![]
                } else {
                    c.find(&d.id).cloned().into_iter().collect()
                }
            } else {
                stack.clone()
            };
            let keys = visible
                .iter()
                .map(|n| text(&n["id"]))
                .collect::<HashSet<_>>();
            for key in surface
                .cards
                .keys()
                .filter(|key| !keys.contains(*key))
                .cloned()
                .collect::<Vec<_>>()
            {
                if let Some(card) = surface.cards.remove(&key) {
                    surface.list.remove(&card.widget);
                }
                surface.snapshots.remove(&key);
                surface.states.remove(&key);
            }
            let mut previous: Option<gtk::Widget> = None;
            for n in &visible {
                let key = text(&n["id"]);
                let mut snapshot = crate::controller::presentation(n, &c.config);
                if snapshot["type"] == "auth-code" {
                    let code = snapshot["data"]["code"].clone();
                    if let Some(actions) = snapshot["actions"].as_array_mut() {
                        for action in actions {
                            if action["copy"] == code {
                                action["label"] = json!("");
                            }
                        }
                    }
                }
                snapshot.as_object_mut().unwrap().remove("remaining");
                let recording = self
                    .executor
                    .capture
                    .as_ref()
                    .filter(|r| r.id == key)
                    .map(|r| r.state.clone())
                    .unwrap_or_default();
                let state = ViewState {
                    locked: c.locked(),
                    floating: detached.is_some(),
                    show_when_locked: c.config["privacy"]["lockScreen"] == "show",
                    paused: c.paused,
                    selected: c.selected == key,
                    blocked: c.blocked(n),
                    busy: self.busy.contains(&key),
                    group_count: n["groupCount"].as_u64().unwrap_or(1) as usize,
                    feedback: self.feedback.get(&key).cloned().unwrap_or_default(),
                    playback: self
                        .executor
                        .playback
                        .get(&key)
                        .cloned()
                        .unwrap_or_default(),
                    recording,
                    ..Default::default()
                };
                let state_key = format!("{state:?}");
                if !surface.cards.contains_key(&key) {
                    let tx = self.event_tx.clone();
                    let card = Rc::new(Card::new(
                        Notice(snapshot.clone()),
                        state.clone(),
                        &self.root,
                        now() as f64,
                        move |event| {
                            let _ = tx.send(event);
                        },
                    ));
                    surface.list.append(&card.widget);
                    surface.cards.insert(key.clone(), card);
                    surface.snapshots.insert(key.clone(), snapshot.clone());
                    surface.states.insert(key.clone(), state_key.clone());
                }
                let card = &surface.cards[&key];
                if surface.snapshots.get(&key) != Some(&snapshot) {
                    card.update(Notice(snapshot.clone()));
                    surface.snapshots.insert(key.clone(), snapshot);
                }
                if surface.states.get(&key) != Some(&state_key) {
                    card.set_state(state);
                    surface.states.insert(key.clone(), state_key);
                }
                card.tick(now() as f64);
                surface
                    .list
                    .reorder_child_after(&card.widget, previous.as_ref());
                previous = Some(card.widget.clone().upcast());
            }
            surface
                .window
                .set_default_size(style["width"].as_i64().unwrap_or(420) as i32, -1);
            surface
                .list
                .set_spacing(style["gap"].as_i64().unwrap_or(10) as i32);
            if visible.is_empty() {
                surface.window.set_visible(false);
            } else {
                if surface.window.is_layer_window() {
                    let selected_here = surface.cards.contains_key(&c.selected);
                    if selected_here && !surface.keyboard_selected {
                        tjik_gtk::surface::keyboard(&surface.window, true);
                    } else if !selected_here {
                        surface.window.set_keyboard_mode(
                            if surface.cards.keys().any(|key| c.held.contains(key)) {
                                KeyboardMode::OnDemand
                            } else {
                                KeyboardMode::None
                            },
                        );
                    }
                    surface.keyboard_selected = selected_here;
                    let monitor_name = detached.map(|d| d.monitor.as_str()).unwrap_or(name);
                    let Some(m) = c.monitors.iter().find(|m| m["name"] == monitor_name) else {
                        continue;
                    };
                    let p = placement(
                        &c.config,
                        monitor_name,
                        m["index"].as_u64().unwrap_or(1) as usize - 1,
                    );
                    let (x, y) = geometry(
                        &p,
                        m["width"].as_i64().unwrap_or(1920) as i32,
                        m["height"].as_i64().unwrap_or(1080) as i32,
                        surface.window.default_width(),
                        surface.window.height().max(1),
                        m["scale"].as_f64().unwrap_or(1.),
                    );
                    let (x, y) = detached.map(|d| (d.x, d.y)).unwrap_or((x, y));
                    surface.window.set_margin(Edge::Left, x);
                    surface.window.set_margin(Edge::Top, y);
                }
                surface.window.set_visible(true);
            }
        }
    }
}
fn popup(
    app: &gtk::Application,
    monitor: &gdk::Monitor,
    config: &Value,
    _event_tx: &mpsc::Sender<CardEvent>,
    controller: Rc<RefCell<Controller>>,
) -> Surface {
    let geometry = monitor.geometry();
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Notjik notifications")
        .resizable(false)
        .decorated(false)
        .build();
    window.add_css_class("notjik");
    window.add_css_class("notjik-popup");
    if std::env::var_os("TJIK_GTK_WINDOWED").is_none() {
        window.init_layer_shell();
        window.set_namespace(Some("notjik"));
        window.set_layer(Layer::Overlay);
        window.set_exclusive_zone(-1);
        window.set_monitor(Some(monitor));
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_anchor(Edge::Left, true);
        window.set_anchor(Edge::Top, true);
    }
    let list = primitives::column(config["style"]["gap"].as_i64().unwrap_or(10) as i32);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(
        (geometry.height() as f64 * config["stack"]["heightPercent"].as_f64().unwrap_or(75.) / 100.)
            as i32,
    );
    scroll.set_child(Some(&list));
    window.set_child(Some(&scroll));
    let keys = gtk::EventControllerKey::new();
    let weak_window = window.downgrade();
    keys.connect_key_pressed(move |_, key, _, _| {
        // Editors retain text navigation and submit keys; Escape always releases selection.
        let editing = weak_window
            .upgrade()
            .and_then(|w| gtk::prelude::GtkWindowExt::focus(&w))
            .is_some_and(|w| {
                w.is::<gtk::TextView>() || w.is::<gtk::Text>() || w.is::<gtk::Entry>()
            });
        let request = match key {
            gdk::Key::Escape => json!({"op":"unfocus"}),
            _ if editing => return glib::Propagation::Proceed,
            gdk::Key::Up => json!({"op":"navigate","direction":"previous"}),
            gdk::Key::Down => json!({"op":"navigate","direction":"next"}),
            gdk::Key::Return | gdk::Key::KP_Enter => json!({"op":"selected","action":"primary"}),
            gdk::Key::Delete | gdk::Key::BackSpace => json!({"op":"selected","action":"dismiss"}),
            _ => return glib::Propagation::Proceed,
        };
        let _ = controller.borrow_mut().handle(request, now());
        glib::Propagation::Stop
    });
    window.add_controller(keys);
    Surface {
        window,
        list,
        cards: HashMap::new(),
        snapshots: HashMap::new(),
        states: HashMap::new(),
        keyboard_selected: false,
    }
}
fn placement(config: &Value, name: &str, index: usize) -> Value {
    let mut p = config["placement"].clone();
    for patch in [
        &config["placement"]["perMonitor"][(index + 1).to_string()],
        &config["placement"]["perMonitor"][name],
    ] {
        if patch.is_object() {
            crate::util::merge(&mut p, patch);
        }
    }
    p
}
fn geometry(p: &Value, sw: i32, sh: i32, w: i32, h: i32, scale: f64) -> (i32, i32) {
    let margin = p["margin"].as_i64().unwrap_or(16) as i32;
    let edge = p["edge"].as_str().unwrap_or("b");
    let position = p["position"].as_str().unwrap_or("c");
    let align = |extent: i32, size: i32| match position {
        "l" => margin,
        "r" => extent - size - margin,
        _ => (extent - size) / 2,
    };
    let (x, y) = if ["t", "b"].contains(&edge) {
        (
            align(sw, w),
            if edge == "t" { margin } else { sh - h - margin },
        )
    } else {
        (
            if edge == "l" { margin } else { sw - w - margin },
            align(sh, h),
        )
    };
    let offset = |v: &Value, extent: i32| {
        v.as_str()
            .and_then(|s| s.strip_suffix('%'))
            .and_then(|s| s.parse::<f64>().ok())
            .map(|n| extent as f64 * n / 100.)
            .unwrap_or(v.as_f64().unwrap_or(0.) / scale) as i32
    };
    (
        (x + offset(&p["offsetX"], sw)).clamp(0, (sw - w).max(0)),
        (y + offset(&p["offsetY"], sh)).clamp(0, (sh - h).max(0)),
    )
}
pub fn run() -> glib::ExitCode {
    unsafe {
        libc::umask(0o077);
    }
    let app = gtk::Application::builder()
        .application_id("org.tjikos.notjik")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let failed = Rc::new(std::cell::Cell::new(false));
    let failure = failed.clone();
    app.connect_activate(move |app| {
        let root = std::env::var_os("NOTJIK_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        if let Err(e) = Host::new(app, root) {
            eprintln!("notjik: {e:#}");
            failure.set(true);
            app.quit();
        }
    });
    for signal in [libc::SIGTERM, libc::SIGINT] {
        let app = app.clone();
        glib::unix_signal_add_local(signal, move || {
            app.quit();
            glib::ControlFlow::Break
        });
    }
    let result = app.run_with_args::<&str>(&[]);
    if failed.get() {
        glib::ExitCode::FAILURE
    } else {
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn placement_offsets_and_bounds() {
        assert_eq!(
            geometry(
                &json!({"edge":"b","position":"r","margin":16,"offsetX":"-10%","offsetY":-24}),
                1000,
                800,
                400,
                200,
                2.
            ),
            (484, 572)
        );
    }
}
