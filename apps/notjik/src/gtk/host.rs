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
    list: super::stack::Stack,
    scene: gtk::Fixed,
    cards: HashMap<String, Rc<Card>>,
    snapshots: HashMap<String, Value>,
    states: HashMap<String, String>,
    keyboard_selected: bool,
}
struct Detached {
    monitor: String,
    workspace: Option<i64>,
    id: String,
    x: i32,
    y: i32,
}
struct Drag {
    id: String,
    source: String,
    preview: String,
    origin_x: i32,
    origin_y: i32,
    height: i32,
}
struct Host {
    root: PathBuf,
    controller: Rc<RefCell<Controller>>,
    bus: Option<Bus>,
    socket: Option<ipc::Server>,
    requests: mpsc::Receiver<ipc::Request>,
    backend: Option<backend_channel::Client>,
    pending: VecDeque<Value>,
    events: mpsc::Receiver<(String, CardEvent)>,
    event_tx: mpsc::Sender<(String, CardEvent)>,
    recording_id: Rc<RefCell<Option<String>>>,
    executor: Executor,
    windows: HashMap<String, Surface>,
    detached: HashMap<String, Detached>,
    drag: Option<Drag>,
    feedback: HashMap<String, String>,
    busy: HashSet<String>,
    provider: Option<gtk::CssProvider>,
    surface_provider: Option<gtk::CssProvider>,
    style: Value,
    last: Instant,
    monitor_tick: Instant,
    desktop: mpsc::Receiver<Value>,
    workspaces: HashMap<String, i64>,
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
        let recording_id = Rc::new(RefCell::new(None));
        let (desktop_tx, desktop) = mpsc::sync_channel(2);
        std::thread::spawn(move || loop {
            let mut v = crate::util::run(
                std::process::Command::new("hyprctl").args(["-j", "activewindow"]),
                Duration::from_secs(2),
            )
            .ok()
            .and_then(|v| serde_json::from_slice::<Value>(&v).ok())
            .unwrap_or(json!({}));
            if let Ok(bytes) = crate::util::run(
                std::process::Command::new("hyprctl").args(["-j", "monitors"]),
                Duration::from_secs(2),
            ) {
                if let Ok(monitors) = serde_json::from_slice::<Value>(&bytes) {
                    v["monitors"] = monitors;
                }
            }
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
            recording_id,
            executor: Executor::new(),
            windows: HashMap::new(),
            detached: HashMap::new(),
            drag: None,
            feedback: HashMap::new(),
            busy: HashSet::new(),
            provider: None,
            surface_provider: None,
            style: Value::Null,
            last: Instant::now(),
            monitor_tick: Instant::now() - Duration::from_secs(2),
            desktop,
            workspaces: HashMap::new(),
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
        glib::timeout_add_local(Duration::from_millis(16), move || {
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
            if let Some(monitors) = desktop["monitors"].as_array() {
                self.workspaces = monitors
                    .iter()
                    .filter_map(|m| {
                        let special = m["specialWorkspace"]["id"].as_i64().unwrap_or(0);
                        let workspace = if special != 0 {
                            Some(special)
                        } else {
                            m["activeWorkspace"]["id"].as_i64()
                        };
                        Some((m["name"].as_str()?.to_string(), workspace?))
                    })
                    .collect();
            }
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
        while let Ok((source, event)) = self.events.try_recv() {
            let id = event.id.clone();
            if let Err(e) = self.event(&source, event) {
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
        if self.drag.as_ref().is_some_and(|d| {
            self.controller.borrow().locked()
                || self.controller.borrow().concealed()
                || !active.contains(&d.id)
                || !self.windows.contains_key(&d.source)
        }) {
            if let Some(d) = self.drag.take() {
                self.controller.borrow_mut().hold(&d.id, false);
                if let Some(source) = self.windows.get(&d.source) {
                    source.list.grab(false);
                }
                if d.preview != d.source {
                    self.detached.remove(&d.preview);
                    if let Some(surface) = self.windows.remove(&d.preview) {
                        surface.window.close();
                    }
                }
            }
        }
        self.feedback.retain(|id, _| active.contains(id));
        self.busy.retain(|id| active.contains(id));
        self.render();
    }
    fn event(&mut self, source: &str, event: CardEvent) -> Result<()> {
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
            Intent::ControlDismiss => c.control_dismiss(&id),
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
                anyhow::ensure!([1., 1.25, 1.5, 2.].contains(&rate), "Invalid playback rate");
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
                if self.drag.is_none() {
                    if finished || dx.hypot(dy) < 8. {
                        return Ok(());
                    }
                    let surface = self
                        .windows
                        .get(source)
                        .context("Drag surface disappeared")?;
                    let card = surface.cards.get(&id).context("Drag card disappeared")?;
                    let monitor = surface
                        .window
                        .monitor()
                        .context("Drag monitor disappeared")?;
                    let origin = card
                        .widget
                        .compute_point(&surface.window, &gtk::graphene::Point::new(0., 0.))
                        .context("Drag origin unavailable")?;
                    let monitor_name = self
                        .detached
                        .get(source)
                        .map(|d| d.monitor.clone())
                        .unwrap_or_else(|| source.to_string());
                    let preview = if self.detached.contains_key(source) {
                        source.to_string()
                    } else {
                        format!("floating:{id}:{}", now())
                    };
                    let x = surface.window.margin(Edge::Left) + origin.x() as i32;
                    let y = surface.window.margin(Edge::Top) + origin.y() as i32;
                    let height = card.widget.height();
                    self.detached.insert(
                        preview.clone(),
                        Detached {
                            workspace: self.workspaces.get(&monitor_name).copied(),
                            monitor: monitor_name,
                            id: id.clone(),
                            x,
                            y,
                        },
                    );
                    if preview != source {
                        self.windows.insert(
                            preview.clone(),
                            popup(
                                &self.app,
                                &monitor,
                                &c.config,
                                &self.event_tx,
                                self.controller.clone(),
                                &preview,
                                self.recording_id.clone(),
                            ),
                        );
                    }
                    self.drag = Some(Drag {
                        id: id.clone(),
                        source: source.into(),
                        preview,
                        origin_x: x,
                        origin_y: y,
                        height,
                    });
                    c.hold(&id, true);
                }
                let drag = self.drag.as_ref().context("Drag unavailable")?;
                if drag.id != id || drag.source != source {
                    return Ok(());
                }
                if let Some(d) = self.detached.get_mut(&drag.preview) {
                    d.x = drag.origin_x + dx.round() as i32;
                    d.y = drag.origin_y + dy.round() as i32;
                    if finished {
                        d.workspace = self.workspaces.get(&d.monitor).copied();
                        if let Some(m) = c.monitors.iter().find(|m| m["name"] == d.monitor) {
                            d.x = d.x.clamp(
                                0,
                                (m["width"].as_i64().unwrap_or(1920) as i32
                                    - c.config["style"]["width"].as_i64().unwrap_or(420) as i32)
                                    .max(0),
                            );
                            d.y = d.y.clamp(
                                0,
                                (m["height"].as_i64().unwrap_or(1080) as i32 - drag.height).max(0),
                            );
                        }
                    }
                }
                if finished {
                    let drag = self.drag.take().unwrap();
                    if let Some(card) = self
                        .windows
                        .get(&drag.preview)
                        .and_then(|s| s.cards.get(&id))
                    {
                        card.set_drag_preview(false);
                    }
                    c.floating.insert(id.clone());
                    c.hold(&id, false);
                    if drag.source != drag.preview && self.detached.remove(&drag.source).is_some() {
                        if let Some(surface) = self.windows.remove(&drag.source) {
                            surface.window.close();
                        }
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
                    name.clone(),
                    popup(
                        &self.app,
                        &monitor,
                        &config,
                        &self.event_tx,
                        self.controller.clone(),
                        &name,
                        self.recording_id.clone(),
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
        *self.recording_id.borrow_mut() = self
            .executor
            .capture
            .as_ref()
            .filter(|capture| {
                !["sending", "discarding", "sent"].contains(&capture.state.phase.as_str())
            })
            .map(|capture| capture.id.clone());
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
        let drag_card = self
            .drag
            .as_ref()
            .and_then(|d| self.windows.get(&d.source).and_then(|s| s.cards.get(&d.id)))
            .cloned();
        let mut stack = c.visible();
        stack.reverse();
        for (name, surface) in &mut self.windows {
            let detached = self.detached.get(name);
            if detached.is_some_and(|d| {
                d.workspace.is_some() && self.workspaces.get(&d.monitor).copied() != d.workspace
            }) && !self.drag.as_ref().is_some_and(|d| d.preview == *name)
            {
                surface.window.set_visible(false);
                for card in surface.cards.values() {
                    card.set_hovered(false);
                }
                continue;
            }
            let visible = if let Some(d) = detached {
                if c.concealed() {
                    vec![]
                } else {
                    c.find(&d.id).cloned().into_iter().collect()
                }
            } else {
                stack.clone()
            };
            if c.concealed() {
                surface.list.clear();
            }
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
                    surface.list.remove(
                        &card.widget,
                        !c.concealed() && !c.locked() && !c.floating.contains(&key),
                    );
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
                    let source = name.clone();
                    let card = Rc::new(Card::new(
                        Notice(snapshot.clone()),
                        state.clone(),
                        &self.root,
                        now() as f64,
                        move |event| {
                            let _ = tx.send((source.clone(), event));
                        },
                    ));
                    if self.drag.as_ref().is_some_and(|d| d.preview == *name) {
                        if let Some(source) = &drag_card {
                            card.copy_interaction_from(source);
                        }
                    }
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
                let drag_source = self
                    .drag
                    .as_ref()
                    .is_some_and(|d| d.source == *name && d.preview != *name && d.id == key);
                card.widget.set_opacity(if drag_source { 0. } else { 1. });
                if self.drag.as_ref().is_some_and(|d| d.preview == *name) {
                    card.set_drag_preview(true);
                }
                card.tick(now() as f64);
                surface
                    .list
                    .reorder_child_after(&card.widget, previous.as_ref());
                previous = Some(card.widget.clone().upcast());
            }
            let card_width = style["width"].as_i64().unwrap_or(420) as i32;
            if visible.is_empty() && !surface.list.has_faces() {
                surface.window.set_visible(false);
            } else {
                if surface.window.is_layer_window() {
                    let preview = self.drag.as_ref().is_some_and(|d| d.preview == *name);
                    let selected_here = !preview && surface.cards.contains_key(&c.selected);
                    if preview {
                        surface.window.set_keyboard_mode(KeyboardMode::None);
                    } else if selected_here && !surface.keyboard_selected {
                        tjik_gtk::surface::keyboard(&surface.window, true);
                    } else if !selected_here {
                        surface.window.set_keyboard_mode(
                            if surface.cards.keys().any(|key| {
                                c.held.contains(key)
                                    && !self.drag.as_ref().is_some_and(|d| d.id == *key)
                            }) {
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
                    let scene_height = ((m["height"].as_f64().unwrap_or(1080.)
                        - 2. * p["margin"].as_f64().unwrap_or(16.))
                        * c.config["stack"]["heightPercent"].as_f64().unwrap_or(75.)
                        / 100.) as i32;
                    let edge = p["edge"].as_str().unwrap_or("b");
                    let position = p["position"].as_str().unwrap_or("c");
                    let align = if edge == "t" || (["l", "r"].contains(&edge) && position == "l") {
                        0.
                    } else if ["l", "r"].contains(&edge) && position == "c" {
                        0.5
                    } else {
                        1.
                    };
                    let (x, y) = geometry(
                        &p,
                        m["width"].as_i64().unwrap_or(1920) as i32,
                        m["height"].as_i64().unwrap_or(1080) as i32,
                        card_width,
                        scene_height.max(1),
                        m["scale"].as_f64().unwrap_or(1.),
                    );
                    let (x, y) = detached.map(|d| (d.x, d.y)).unwrap_or((x, y));
                    surface.list.configure(
                        card_width,
                        scene_height,
                        style["gap"].as_i64().unwrap_or(10) as i32,
                        if detached.is_some() { 0. } else { align },
                        if self.drag.as_ref().is_some_and(|d| d.preview == *name) {
                            0
                        } else {
                            c.config["stack"]["animationMs"].as_u64().unwrap_or(220)
                        },
                        (m["height"].as_i64().unwrap_or(1080) as i32 - y - scene_height)
                            .clamp(0, 32),
                    );
                    surface.window.set_default_size(
                        m["width"].as_i64().unwrap_or(1920) as i32,
                        m["height"].as_i64().unwrap_or(1080) as i32,
                    );
                    surface.scene.move_(&surface.list, x as f64, y as f64);
                    surface.list.position(
                        x,
                        y,
                        !self
                            .drag
                            .as_ref()
                            .is_some_and(|d| d.preview == *name && d.source != *name),
                    );
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
    event_tx: &mpsc::Sender<(String, CardEvent)>,
    controller: Rc<RefCell<Controller>>,
    surface_name: &str,
    recording_id: Rc<RefCell<Option<String>>>,
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
    let list = super::stack::Stack::new();
    list.configure(
        config["style"]["width"].as_i64().unwrap_or(420) as i32,
        (geometry.height() as f64 * config["stack"]["heightPercent"].as_f64().unwrap_or(75.) / 100.)
            as i32,
        config["style"]["gap"].as_i64().unwrap_or(10) as i32,
        1.,
        config["stack"]["animationMs"].as_u64().unwrap_or(220),
        0,
    );
    let scene = gtk::Fixed::new();
    scene.put(&list, 0., 0.);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&gtk::Box::new(gtk::Orientation::Vertical, 0)));
    overlay.add_overlay(&scene);
    overlay.set_measure_overlay(&scene, false);
    window.set_child(Some(&overlay));
    window.set_default_size(geometry.width(), geometry.height());
    // Layer surfaces do not provide a widget-local grab outside a card's bounds.
    // Keep pointer ownership on the stable monitor-sized native surface instead.
    let pointer = gtk::EventControllerLegacy::new();
    pointer.set_propagation_phase(gtk::PropagationPhase::Capture);
    let press = Rc::new(RefCell::new(None::<(String, f64, f64, bool)>));
    let weak = window.downgrade();
    let input = list.clone();
    let tx = event_tx.clone();
    let source = surface_name.to_string();
    let owner = controller.clone();
    let handler = Rc::new(move |event: &gdk::Event| {
        let Some((x, y)) = event.position() else {
            return glib::Propagation::Proceed;
        };
        let emit = |id: &str, intent| {
            let _ = tx.send((
                source.clone(),
                CardEvent {
                    id: id.into(),
                    intent,
                },
            ));
        };
        match event.event_type() {
            gdk::EventType::ButtonPress => {
                if event
                    .downcast_ref::<gdk::ButtonEvent>()
                    .is_none_or(|b| b.button() != 1)
                    || event
                        .modifier_state()
                        .contains(gdk::ModifierType::SHIFT_MASK)
                {
                    return glib::Propagation::Proceed;
                }
                let Some(window) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                let mut picked = window.pick(x, y, gtk::PickFlags::DEFAULT);
                let mut id = None;
                while let Some(widget) = picked {
                    if widget.is::<gtk::Button>()
                        || widget.is::<gtk::TextView>()
                        || widget.is::<gtk::Range>()
                        || widget.is::<gtk::Entry>()
                    {
                        return glib::Propagation::Proceed;
                    }
                    if let Some(key) = widget.widget_name().strip_prefix("card-") {
                        id = Some(key.to_string());
                        break;
                    }
                    picked = widget.parent();
                }
                let Some(id) = id else {
                    return glib::Propagation::Proceed;
                };
                if !owner
                    .borrow()
                    .find(&id)
                    .is_some_and(|n| owner.borrow().allowed(n))
                {
                    return glib::Propagation::Proceed;
                }
                input.grab(true);
                *press.borrow_mut() = Some((id, x, y, false));
                glib::Propagation::Stop
            }
            gdk::EventType::MotionNotify => {
                let mut current = press.borrow_mut();
                let Some((id, ox, oy, active)) = current.as_mut() else {
                    return glib::Propagation::Proceed;
                };
                let (dx, dy) = (x - *ox, y - *oy);
                if *active || dx.hypot(dy) >= 8. {
                    *active = true;
                    emit(
                        id,
                        Intent::Drag {
                            dx,
                            dy,
                            finished: false,
                        },
                    );
                }
                glib::Propagation::Stop
            }
            gdk::EventType::ButtonRelease => {
                if event
                    .downcast_ref::<gdk::ButtonEvent>()
                    .is_none_or(|b| b.button() != 1)
                {
                    return glib::Propagation::Proceed;
                }
                let Some((id, ox, oy, active)) = press.borrow_mut().take() else {
                    return glib::Propagation::Proceed;
                };
                input.grab(false);
                if active {
                    emit(
                        &id,
                        Intent::Drag {
                            dx: x - ox,
                            dy: y - oy,
                            finished: true,
                        },
                    );
                } else if !owner.borrow().floating.contains(&id) {
                    emit(&id, Intent::Dismiss);
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    let capture = handler.clone();
    pointer.connect_event(move |_, event| capture(event));
    window.add_controller(pointer);
    let target = gtk::EventControllerLegacy::new();
    target.set_propagation_phase(gtk::PropagationPhase::Target);
    target.connect_event(move |_, event| handler(event));
    window.add_controller(target);
    let cancel_key = Rc::new(RefCell::new(super::keyboard::CancelKeyTap::default()));
    let cancel_keys = gtk::EventControllerKey::new();
    cancel_keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let key_state = cancel_key.clone();
    cancel_keys.connect_key_pressed(move |_, key, _, _| {
        key_state.borrow_mut().press(key);
        glib::Propagation::Proceed
    });
    let key_state = cancel_key;
    let recording = recording_id;
    let tx = event_tx.clone();
    let source = surface_name.to_string();
    cancel_keys.connect_key_released(move |_, key, _, _| {
        if key_state.borrow_mut().release(key) {
            if let Some(id) = recording.borrow().clone() {
                let _ = tx.send((
                    source.clone(),
                    CardEvent {
                        id,
                        intent: Intent::RecordDiscard,
                    },
                ));
            }
        }
    });
    window.add_controller(cancel_keys);
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
        scene,
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
