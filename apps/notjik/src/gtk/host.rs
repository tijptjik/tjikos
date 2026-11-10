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
    native: bool,
    window: gtk::ApplicationWindow,
    list: super::stack::Stack,
    scene: gtk::Fixed,
    cards: HashMap<String, Rc<Card>>,
    snapshots: HashMap<String, Value>,
    states: HashMap<String, String>,
    keyboard_selected: bool,
    keyboard_was_active: bool,
    pointer_keyboard_until: Rc<std::cell::Cell<Option<Instant>>>,
    keyboard_release_until: Rc<std::cell::Cell<Option<Instant>>>,
}
impl Surface {
    fn make_native(
        &mut self,
        app: &gtk::Application,
        title: &str,
        source: &str,
        id: &str,
        events: &mpsc::Sender<(String, CardEvent)>,
    ) {
        let native = gtk::ApplicationWindow::builder()
            .application(app)
            .title(title)
            .decorated(false)
            .resizable(false)
            .build();
        native.add_css_class("notjik");
        native.add_css_class("notjik-popup");
        native.add_css_class("notjik-native-detached");
        let width = self.list.measure(gtk::Orientation::Horizontal, -1).1.max(1);
        let height = self
            .cards
            .values()
            .map(|c| c.detached_height(width))
            .max()
            .unwrap_or(1)
            .max(1);
        // Freeze the already rendered face until the new native surface paints.
        let snapshot = gtk::Snapshot::new();
        let scale = self.window.scale_factor().max(1) as f32;
        snapshot.scale(scale, scale);
        gtk::WidgetPaintable::new(Some(&self.list)).snapshot(
            &snapshot,
            width as f64,
            self.list.height() as f64,
        );
        if let (Some(node), Some(renderer)) = (snapshot.to_node(), self.window.renderer()) {
            let texture = renderer.render_texture(
                &node,
                Some(&gtk::graphene::Rect::new(
                    0.,
                    0.,
                    width as f32 * scale,
                    height as f32 * scale,
                )),
            );
            let face = gtk::Picture::for_paintable(&texture);
            face.set_content_fit(gtk::ContentFit::Fill);
            face.set_size_request(width, height);
            let position = self.scene.child_position(&self.list);
            self.scene.put(&face, position.0, position.1);
        }
        self.scene.remove(&self.list);
        self.list.configure(width, height, 0, 0., 0, 0);
        self.list.position(0, 0, true);
        self.list.grab(false);
        native.set_child(Some(&self.list));
        native.set_default_size(width, height);
        let send = events.clone();
        let source = source.to_owned();
        let id = id.to_owned();
        native.connect_close_request(move |_| {
            let _ = send.send((
                source.clone(),
                CardEvent {
                    id: id.clone(),
                    intent: Intent::Dismiss,
                },
            ));
            glib::Propagation::Proceed
        });
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        let weak = native.downgrade();
        drag.connect_drag_begin(move |gesture, x, y| {
            if gesture
                .current_event_state()
                .intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK)
            {
                gesture.set_state(gtk::EventSequenceState::Denied);
                return;
            }
            let Some(window) = weak.upgrade() else { return };
            let mut picked = window.pick(x, y, gtk::PickFlags::DEFAULT);
            while let Some(widget) = picked {
                if widget.is::<gtk::Button>()
                    || widget.is::<gtk::Editable>()
                    || widget.is::<gtk::TextView>()
                    || widget.is::<gtk::Range>()
                    || widget.is::<gtk::DropDown>()
                    || widget
                        .downcast_ref::<gtk::Label>()
                        .is_some_and(|l| l.is_selectable())
                {
                    gesture.set_state(gtk::EventSequenceState::Denied);
                    return;
                }
                picked = widget.parent();
            }
            if let (Some(surface), Some(event)) = (
                window.surface().and_downcast::<gdk::Toplevel>(),
                gesture.current_event(),
            ) {
                if let Some(device) = event.device() {
                    surface.begin_move(&device, 1, x, y, event.time());
                }
            }
        });
        native.add_controller(drag);
        let old = self.window.clone();
        let scene = self.scene.clone();
        let done: Rc<dyn Fn()> = Rc::new(move || {
            old.close();
            while let Some(child) = scene.first_child() {
                scene.remove(&child);
            }
        });
        let painted = done.clone();
        native.connect_map(move |window| {
            if let Some(clock) = window.frame_clock() {
                let signal = Rc::new(RefCell::new(None));
                let slot = signal.clone();
                let done = painted.clone();
                *signal.borrow_mut() = Some(clock.connect_after_paint(move |clock| {
                    done();
                    if let Some(id) = slot.borrow_mut().take() {
                        clock.disconnect(id);
                    }
                }));
            }
        });
        glib::timeout_add_local_once(Duration::from_millis(250), move || done());
        native.set_visible(true);
        self.window = native;
        self.native = true;
    }
}

// Release temporary placement rules even if a card disappears while the worker runs.
struct PreparedWindow(String);
impl Drop for PreparedWindow {
    fn drop(&mut self) {
        let title = self.0.clone();
        std::thread::spawn(move || {
            let _ = tjik_desktop::selected().release_toplevel(&title);
        });
    }
}
struct Detached {
    preparation: Option<mpsc::Receiver<tjik_desktop::Result<PreparedWindow>>>,
    monitor: String,
    workspace: Option<String>,
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
    source_monitor: String,
    monitor_x: i32,
    monitor_y: i32,
}
#[derive(Clone)]
struct PointerPress {
    id: String,
    source: String,
    x: f64,
    y: f64,
    active: bool,
    control: bool,
    shift: bool,
    native: Option<gdk::Drag>,
    monitor_x: f64,
    monitor_y: f64,
}
struct Host {
    root: PathBuf,
    showcase: Option<super::showcase::Showcase>,
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
    desktop: mpsc::Receiver<tjik_desktop::Subscription>,
    desktop_events: Option<tjik_desktop::Subscription>,
    workspaces: HashMap<String, String>,
    all_monitors: Vec<Value>,
    retained: HashMap<String, Rc<Card>>,
    pointer: Rc<RefCell<Option<PointerPress>>>,
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
        // Resolve/connect on a worker; the UI receives the bounded subscription once.
        // EventSource already coalesces full snapshots, so no second event queue or
        // polling worker is needed for the lifetime of the host.
        std::thread::spawn(move || {
            if let Ok(events) = tjik_desktop::selected().subscribe() {
                let _ = desktop_tx.send(events);
            }
        });
        let host = Rc::new(RefCell::new(Self {
            showcase: (std::env::var_os("NOTJIK_SYNTHETIC").is_some()
                && root.join(".synthetic-only").is_file())
            .then(|| super::showcase::Showcase::new(app, &controller.borrow().config)),
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
            desktop_events: None,
            workspaces: HashMap::new(),
            all_monitors: vec![],
            retained: HashMap::new(),
            pointer: Rc::new(RefCell::new(None)),
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
        self.desktop_events.take();
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
        if let Ok(events) = self.desktop.try_recv() {
            self.desktop_events = Some(events);
        }
        let desktop_event = self
            .desktop_events
            .as_ref()
            .map(|events| events.recv_timeout(Duration::ZERO));
        if let Some(Err(_)) = desktop_event {
            self.desktop_events.take();
            self.workspaces.clear();
            self.controller.borrow_mut().focused_app.clear();
        } else if let Some(Ok(Some(event))) = desktop_event {
            match event.state {
                tjik_desktop::DesktopState::Ready(snapshot) => {
                    self.workspaces = snapshot
                        .outputs
                        .iter()
                        .filter_map(|output| {
                            Some((output.name.clone(), output.workspace.as_ref()?.id.clone()))
                        })
                        .collect();
                    self.controller.borrow_mut().focused_app = snapshot
                        .windows
                        .iter()
                        .find(|window| Some(&window.id) == snapshot.focused_window.as_ref())
                        .and_then(|window| window.app_id.clone())
                        .unwrap_or_default();
                }
                tjik_desktop::DesktopState::Unavailable(_) => {
                    self.workspaces.clear();
                    self.controller.borrow_mut().focused_app.clear();
                }
            }
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
        if let Some(showcase) = &mut self.showcase {
            showcase.tick();
        }
        while let Ok(request) = self.requests.try_recv() {
            let result = self
                .request(request.value)
                .unwrap_or_else(|e| json!({"ok":false,"error":e.to_string()}));
            let _ = request.reply.send(result);
        }
        // Independent holds can overlap: a completed playback/send or cancelled
        // drag must not allow an open reply composer to expire.
        for card in self
            .windows
            .values()
            .flat_map(|s| s.cards.values())
            .chain(self.retained.values())
        {
            if card.context.composing.get() {
                self.controller.borrow_mut().hold(&card.notice().id(), true);
            }
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
                    if self.showcase.is_none() {
                        self.executor.command(String::new(), "command".into(), args)
                    }
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
        if self.pointer.borrow().as_ref().is_some_and(|p| {
            self.controller.borrow().locked()
                || self.controller.borrow().concealed()
                || !active.contains(&p.id)
                || !self.windows.contains_key(&p.source)
        }) || self.drag.as_ref().is_some_and(|d| {
            self.controller.borrow().locked()
                || self.controller.borrow().concealed()
                || !active.contains(&d.id)
                || !self.windows.contains_key(&d.source)
        }) {
            self.cancel_drag();
        }
        if let Some(showcase) = &mut self.showcase {
            showcase.states.retain(|id, _| active.contains(id));
        }
        self.retained.retain(|id, _| active.contains(id));
        self.feedback.retain(|id, _| active.contains(id));
        self.busy.retain(|id| active.contains(id));
        for surface in self.windows.values_mut() {
            let active = surface.window.is_active();
            if surface.keyboard_was_active && !active && surface.keyboard_selected {
                // Leaving a deliberately selected card ends keyboard ownership;
                // later hover or arrivals must not reacquire it.
                self.controller.borrow_mut().selected.clear();
                if surface.window.is_layer_window() {
                    surface.window.set_keyboard_mode(KeyboardMode::None);
                }
                surface.pointer_keyboard_until.set(None);
            }
            surface.keyboard_was_active = active;
        }
        self.finish_native_windows();
        self.render();
    }
    fn finish_native_windows(&mut self) {
        for (name, detached) in &mut self.detached {
            let result = detached
                .preparation
                .as_ref()
                .and_then(|rx| match rx.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err(tjik_desktop::DesktopError::Cancelled))
                    }
                    Err(mpsc::TryRecvError::Empty) => None,
                });
            let Some(result) = result else { continue };
            detached.preparation = None;
            match result {
                Ok(prepared) => {
                    if let Some(surface) = self.windows.get_mut(name) {
                        surface.make_native(
                            &self.app,
                            &prepared.0,
                            name,
                            &detached.id,
                            &self.event_tx,
                        );
                    }
                    glib::timeout_add_local_once(Duration::from_secs(1), move || {
                        drop(prepared);
                    });
                }
                Err(error) => eprintln!("Notjik native detachment unavailable: {error}"),
            }
        }
    }
    fn request(&mut self, request: Value) -> Result<Value> {
        if request["op"] == "reference-artwork" {
            let c = self.controller.borrow();
            let n = Notice(
                c.find(&text(&request["id"]))
                    .cloned()
                    .context("Notification not found")?,
            );
            let image = if !text(&n.0["image"]).is_empty() {
                text(&n.0["image"]).to_string()
            } else if !text(&n.0["icon"]).is_empty() {
                if text(&n.0["icon"]).starts_with("network-wireless") {
                    crate::util::uri(&self.root.join("assets/icons/wifi-cover.png"))?
                } else {
                    text(&n.0["icon"])
                }
            } else {
                crate::util::uri(&super::artwork_path(&self.root, &n))?
            };
            return Ok(json!({"ok":true,"image":image}));
        }
        if request["op"] == "cycle" {
            let card = self
                .windows
                .values()
                .find_map(|s| s.cards.get(&text(&request["id"])))
                .cloned();
            return self
                .showcase
                .as_mut()
                .context("Synthetic showcase controls are disabled")?
                .control(&self.root, &request, card.as_ref());
        }
        self.controller.borrow_mut().handle(request, now())
    }
    fn cancel_drag(&mut self) {
        let pressed = self.pointer.borrow_mut().take();
        if let Some(native) = pressed.and_then(|p| p.native) {
            native.drop_done(false);
        }
        let Some(d) = self.drag.take() else { return };
        self.controller.borrow_mut().hold(&d.id, false);
        if let Some(source) = self.windows.get(&d.source) {
            source.list.grab(false);
        }
        if self.windows.contains_key(&d.source) {
            let _ = transfer_card(
                &mut self.windows,
                &self.event_tx,
                &d.id,
                &d.preview,
                &d.source,
            );
            if let Some(card) = self.windows[&d.source].cards.get(&d.id) {
                card.set_drag_preview(false);
            }
        }
        self.detached.remove(&d.preview);
        if let Some(surface) = self.windows.remove(&d.preview) {
            surface.window.close();
        }
    }
    fn event(&mut self, source: &str, event: CardEvent) -> Result<()> {
        let id = event.id;
        if matches!(event.intent, Intent::CancelDrag) {
            self.cancel_drag();
            return Ok(());
        }
        if matches!(event.intent, Intent::Drag { .. }) {
            // Outputs can be configured immediately after hotplug. Route with the
            // current GDK geometry rather than the periodic desktop snapshot.
            self.monitors();
        }
        if let Some(showcase) = &mut self.showcase {
            if let Some(card) = self.windows.values().find_map(|s| s.cards.get(&id)) {
                if showcase.event(card, &event.intent) {
                    return Ok(());
                }
            }
        }
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
            Intent::CopyMarkdown => {
                if let Some(display) = gdk::Display::default() {
                    display
                        .clipboard()
                        .set_text(&super::clipboard::markdown(&Notice(n), now() as f64));
                }
            }
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
            Intent::Drag {
                dx,
                dy,
                finished,
                pointer,
            } => {
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
                    let bounds = monitor.geometry();
                    let origin = card
                        .widget
                        .compute_point(&surface.window, &gtk::graphene::Point::new(0., 0.))
                        .context("Drag origin unavailable")?;
                    let monitor_name = self
                        .detached
                        .get(source)
                        .map(|d| d.monitor.clone())
                        .unwrap_or_else(|| source.to_string());
                    let preview = format!("floating:{id}:{}", now());
                    let (x, y) = (origin.x() as i32, origin.y() as i32);
                    let height = card.widget.height();
                    self.detached.insert(
                        preview.clone(),
                        Detached {
                            preparation: None,
                            workspace: self.workspaces.get(&monitor_name).cloned(),
                            monitor: monitor_name.clone(),
                            id: id.clone(),
                            x,
                            y,
                        },
                    );
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
                            self.pointer.clone(),
                        ),
                    );
                    transfer_card(&mut self.windows, &self.event_tx, &id, source, &preview)?;
                    self.windows[&preview].cards[&id].set_drag_preview(true);
                    self.drag = Some(Drag {
                        id: id.clone(),
                        source: source.into(),
                        preview,
                        origin_x: x,
                        origin_y: y,
                        height,
                        source_monitor: monitor_name,
                        monitor_x: bounds.x(),
                        monitor_y: bounds.y(),
                    });
                    c.hold(&id, true);
                }
                let drag = self.drag.as_ref().context("Drag unavailable")?;
                if drag.id != id || drag.source != source {
                    return Ok(());
                }
                let (px, py) =
                    pointer.unwrap_or((drag.origin_x as f64 + dx, drag.origin_y as f64 + dy));
                let global_pointer = (drag.monitor_x as f64 + px, drag.monitor_y as f64 + py);
                let destination = self
                    .all_monitors
                    .iter()
                    .find(|m| {
                        let (x, y, w, h) = (
                            m["x"].as_f64().unwrap_or(0.),
                            m["y"].as_f64().unwrap_or(0.),
                            m["width"].as_f64().unwrap_or(0.),
                            m["height"].as_f64().unwrap_or(0.),
                        );
                        global_pointer.0 >= x
                            && global_pointer.0 < x + w
                            && global_pointer.1 >= y
                            && global_pointer.1 < y + h
                    })
                    .or_else(|| {
                        self.all_monitors
                            .iter()
                            .find(|m| m["name"] == drag.source_monitor)
                    });
                if let (Some(d), Some(m)) = (self.detached.get_mut(&drag.preview), destination) {
                    let name = text(&m["name"]);
                    if d.monitor != name {
                        if let Some(monitor) = find_monitor(&name) {
                            self.windows[&drag.preview]
                                .window
                                .set_monitor(Some(&monitor));
                            d.monitor = name;
                        }
                    }
                    d.x = drag.monitor_x + drag.origin_x + dx.round() as i32
                        - m["x"].as_i64().unwrap_or(0) as i32;
                    d.y = drag.monitor_y + drag.origin_y + dy.round() as i32
                        - m["y"].as_i64().unwrap_or(0) as i32;
                    if finished {
                        d.workspace = self.workspaces.get(&d.monitor).cloned();
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
                if finished {
                    let drag = self.drag.take().unwrap();
                    self.windows[&drag.preview].cards[&id].set_drag_preview(false);
                    if let Some(detached) = self.detached.get_mut(&drag.preview) {
                        let monitor = detached.monitor.clone();
                        let workspace = detached.workspace.clone();
                        let (x, y) = (detached.x, detached.y);
                        let width = c.config["style"]["width"].as_f64().unwrap_or(420.);
                        let height = self.windows[&drag.preview].cards[&id]
                            .detached_height(width as i32)
                            as f64;
                        let title = format!("notjik-detached:{}:{}", std::process::id(), now());
                        let (tx, rx) = mpsc::channel();
                        detached.preparation = Some(rx);
                        std::thread::spawn(move || {
                            let desktop = tjik_desktop::selected();
                            let result = (|| {
                                let snapshot = desktop.snapshot()?;
                                let output = snapshot
                                    .outputs
                                    .iter()
                                    .find(|o| o.name == monitor)
                                    .ok_or(tjik_desktop::DesktopError::StaleTarget)?;
                                let workspace = snapshot
                                    .workspaces
                                    .iter()
                                    .find(|w| {
                                        w.output.as_ref() == Some(&output.id)
                                            && Some(&w.workspace.id) == workspace.as_ref()
                                    })
                                    .ok_or(tjik_desktop::DesktopError::StaleTarget)?;
                                desktop.prepare_toplevel(
                                    &title,
                                    &tjik_desktop::WorkspaceTarget {
                                        generation: snapshot.generation.clone(),
                                        output: output.id.clone(),
                                        id: Some(workspace.workspace.id.clone()),
                                        name: workspace.workspace.name.clone().unwrap_or_default(),
                                    },
                                    tjik_desktop::Rect {
                                        x: output.logical.x + x as f64,
                                        y: output.logical.y + y as f64,
                                        width,
                                        height,
                                    },
                                )?;
                                Ok(PreparedWindow(title.clone()))
                            })();
                            if tx.send(result).is_err() {
                                let _ = desktop.release_toplevel(&title);
                            }
                        });
                    }
                    c.floating.insert(id.clone());
                    c.hold(&id, false);
                    if self.detached.remove(&drag.source).is_some() {
                        if let Some(surface) = self.windows.remove(&drag.source) {
                            surface.window.close();
                        }
                    }
                }
            }
            Intent::Hover(_) | Intent::Hold(_) | Intent::CancelDrag => {}
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
            monitors.push(json!({"index":i+1,"name":name,"x":geometry.x(),"y":geometry.y(),"width":geometry.width(),"height":geometry.height(),"scale":monitor.scale_factor(),"selected":chosen}));
            if chosen {
                selected.insert(name.clone());
            }
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
                        self.pointer.clone(),
                    ),
                );
            }
        }
        let removed = self
            .windows
            .keys()
            .filter(|name| {
                !self.windows.get(*name).is_some_and(|s| s.native)
                    && !monitors.iter().any(|m| m["name"] == **name)
                    && !self
                        .detached
                        .get(*name)
                        .is_some_and(|d| monitors.iter().any(|m| m["name"] == d.monitor))
            })
            .cloned()
            .collect::<Vec<_>>();
        self.retire_surfaces(removed, &selected);
        self.all_monitors = monitors.clone();
        self.controller.borrow_mut().monitors = monitors
            .into_iter()
            .filter(|v| v["selected"] == true)
            .collect();
    }
    fn retire_surfaces(&mut self, removed: Vec<String>, selected: &HashSet<String>) {
        if self
            .pointer
            .borrow()
            .as_ref()
            .is_some_and(|p| removed.contains(&p.source))
            || self
                .drag
                .as_ref()
                .is_some_and(|d| removed.contains(&d.source) || removed.contains(&d.preview))
        {
            self.cancel_drag();
        }
        for name in removed {
            // Rehome the existing widget (including a reply draft) before retiring an output.
            if let Some(destination) = selected
                .iter()
                .find(|n| **n != name && self.windows.contains_key(*n))
                .cloned()
            {
                let ids = self
                    .windows
                    .get(&name)
                    .map(|s| s.cards.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                for id in ids {
                    let _ =
                        transfer_card(&mut self.windows, &self.event_tx, &id, &name, &destination);
                }
            }
            if let Some(surface) = self.windows.remove(&name) {
                for (id, card) in surface.cards {
                    surface.list.remove(&card.widget, false);
                    self.retained.insert(id, card);
                }
                surface.window.close();
            }
            if let Some(d) = self.detached.remove(&name) {
                self.controller.borrow_mut().floating.remove(&d.id);
            }
        }
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
        if c.concealed() || c.locked() {
            self.pointer.borrow_mut().take();
        }
        let pressed = self.pointer.borrow().clone();
        let grab_monitor = pressed
            .as_ref()
            .and_then(|p| self.windows.get(&p.source))
            .and_then(|s| s.window.monitor())
            .and_then(|m| m.connector())
            .map(|s| s.to_string());
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
        let mut stack = c.visible();
        stack.reverse();
        for (name, surface) in &mut self.windows {
            let detached = self.detached.get(name);
            let capture = pressed.as_ref().is_some_and(|p| {
                p.source == *name
                    || (detached.is_none() && grab_monitor.as_deref() != Some(name.as_str()))
            });
            surface.list.grab(capture);
            if !surface.native
                && detached.is_some_and(|d| {
                    d.workspace.is_some() && self.workspaces.get(&d.monitor).cloned() != d.workspace
                })
                && !self.drag.as_ref().is_some_and(|d| d.preview == *name)
            {
                // Preserve the mapped layer surface and card state. Remapping
                // it on workspace return replays the compositor's entrance fade.
                surface.scene.set_visible(false);
                surface.window.set_keyboard_mode(KeyboardMode::None);
                surface.list.position(0, 0, false);
                if let Some(native) = surface.window.surface() {
                    native.set_input_region(&gtk::cairo::Region::create());
                }
                for card in surface.cards.values() {
                    card.set_hovered(false);
                }
                continue;
            }
            surface.scene.set_visible(true);
            if c.concealed() {
                if surface.native {
                    surface.list.set_visible(false);
                    if let Some(native) = surface.window.surface() {
                        native.set_input_region(&gtk::cairo::Region::create());
                    }
                } else {
                    surface.list.clear();
                    surface.window.set_visible(false);
                }
                for card in surface.cards.values() {
                    let mut state = card.context.state.borrow().clone();
                    state.locked = c.locked();
                    state.paused = c.paused;
                    card.set_state(state);
                    card.set_hovered(false);
                }
                surface.states.clear();
                continue;
            }
            surface.list.set_visible(true);
            let visible = if let Some(d) = detached {
                if self.drag.as_ref().is_some_and(|drag| drag.source == *name) {
                    vec![]
                } else {
                    c.find(&d.id).cloned().into_iter().collect()
                }
            } else {
                stack
                    .iter()
                    .filter(|_| c.monitors.iter().any(|m| m["name"] == *name))
                    .filter(|n| !self.drag.as_ref().is_some_and(|d| d.id == text(&n["id"])))
                    .cloned()
                    .collect()
            };
            if c.concealed() {
                surface.list.clear();
            }
            surface.list.set_demo_scroll(
                detached.is_none()
                    && visible
                        .iter()
                        .any(|n| n["app"] == crate::demo::APP && n["data"]["demoMix"] == true),
            );
            let keys = visible
                .iter()
                .map(|n| text(&n["id"]))
                .collect::<HashSet<_>>();
            // A dismissed group member reveals its successor in the same slot.
            // Keep the Card (and its animated notches), snapshot only its old face.
            if !c.concealed() && !c.locked() && detached.is_none() {
                for n in &visible {
                    let key = text(&n["id"]);
                    if surface.cards.contains_key(&key) {
                        continue;
                    }
                    let successor = surface.snapshots.iter().find_map(|(old, snapshot)| {
                        if keys.contains(old) || c.find(old).is_some() {
                            return None;
                        }
                        let members = snapshot["memberIds"].as_array()?;
                        (members.len() > 1 && members.iter().any(|id| id == &n["id"]))
                            .then(|| old.clone())
                    });
                    if let Some(old) = successor {
                        if let Some(card) = surface.cards.remove(&old) {
                            surface.list.replace_group_face(&card.widget);
                            surface.cards.insert(key, card);
                        }
                        surface.snapshots.remove(&old);
                        surface.states.remove(&old);
                    }
                }
            }
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
                let mut state = ViewState {
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
                if let Some(showcase) = &self.showcase {
                    showcase.apply(&key, &mut state);
                }
                let state_key = format!("{state:?}");
                if !surface.cards.contains_key(&key) {
                    let tx = self.event_tx.clone();
                    let source = name.clone();
                    let card = if let Some(card) = self.retained.remove(&key) {
                        card.set_callback(move |event| {
                            let _ = tx.send((source.clone(), event));
                        });
                        card
                    } else {
                        Rc::new(Card::new(
                            Notice(snapshot.clone()),
                            state.clone(),
                            &self.root,
                            now() as f64,
                            move |event| {
                                let _ = tx.send((source.clone(), event));
                            },
                        ))
                    };
                    surface.list.append(&card.widget);
                    surface.cards.insert(key.clone(), card);
                    surface.snapshots.insert(key.clone(), snapshot.clone());
                    surface.states.insert(key.clone(), state_key.clone());
                }
                let card = &surface.cards[&key];
                if card.widget.parent().is_none() {
                    surface.list.append(&card.widget);
                }
                if surface.snapshots.get(&key) != Some(&snapshot) {
                    card.update(Notice(snapshot.clone()));
                    surface.snapshots.insert(key.clone(), snapshot);
                }
                if surface.states.get(&key) != Some(&state_key) {
                    card.set_state(state);
                    surface.states.insert(key.clone(), state_key);
                }
                card.notches
                    .duration(c.config["stack"]["animationMs"].as_u64().unwrap_or(220));
                card.tick(now() as f64);
                surface
                    .list
                    .reorder_child_after(&card.widget, previous.as_ref());
                previous = Some(card.widget.clone().upcast());
            }
            let card_width = style["width"].as_i64().unwrap_or(420) as i32;
            if visible.is_empty()
                && !capture
                && !surface.list.has_faces()
                && !self.drag.as_ref().is_some_and(|d| d.source == *name)
            {
                surface.window.set_visible(false);
            } else {
                if surface.window.is_layer_window() {
                    let preview = self.drag.as_ref().is_some_and(|d| d.preview == *name);
                    let selected_here = !preview && surface.cards.contains_key(&c.selected);
                    if surface.keyboard_selected && !selected_here {
                        surface
                            .keyboard_release_until
                            .set(Some(Instant::now() + Duration::from_millis(100)));
                    }
                    if preview
                        || surface
                            .keyboard_release_until
                            .get()
                            .is_some_and(|until| until > Instant::now())
                    {
                        surface.window.set_keyboard_mode(KeyboardMode::None);
                    } else if selected_here && !surface.keyboard_selected {
                        tjik_gtk::surface::keyboard(&surface.window, true);
                    } else if !selected_here {
                        // Idle arrivals and pointer hover must never acquire keyboard focus.
                        // A short explicit-click lease allows Wayland to deliver modifiers.
                        if !surface
                            .pointer_keyboard_until
                            .get()
                            .is_some_and(|until| until > Instant::now())
                            && self.pointer.borrow().is_none()
                        {
                            surface.window.set_keyboard_mode(KeyboardMode::None);
                        }
                    }
                    surface.keyboard_selected = selected_here;
                    let monitor_name = detached.map(|d| d.monitor.as_str()).unwrap_or(name);
                    let Some(m) = self.all_monitors.iter().find(|m| m["name"] == monitor_name)
                    else {
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
                if surface.native {
                    let height = surface
                        .cards
                        .values()
                        .map(|card| {
                            card.widget
                                .measure(gtk::Orientation::Vertical, card_width)
                                .1
                        })
                        .max()
                        .unwrap_or(1)
                        .max(1);
                    surface.list.configure(card_width, height, 0, 0., 0, 0);
                    surface.list.position(0, 0, true);
                    // Preserve the native origin during footer animation. Native
                    // compositors may center client-requested resizes; reserve the
                    // controls at map time and retain that allocation when hidden.
                    let reserved = surface.window.default_height().max(height);
                    surface.window.set_default_size(card_width, reserved);
                }
                surface.window.set_visible(true);
            }
        }
    }
}
fn find_monitor(name: &str) -> Option<gdk::Monitor> {
    gdk::Display::default()?
        .monitors()
        .iter::<gdk::Monitor>()
        .flatten()
        .find(|m| m.connector().as_deref() == Some(name))
}
fn transfer_card(
    windows: &mut HashMap<String, Surface>,
    tx: &mpsc::Sender<(String, CardEvent)>,
    id: &str,
    source: &str,
    destination: &str,
) -> Result<()> {
    let from = windows
        .get_mut(source)
        .context("Source surface disappeared")?;
    let card = from.cards.remove(id).context("Source card disappeared")?;
    from.list.remove(&card.widget, false);
    from.snapshots.remove(id);
    from.states.remove(id);
    let events = tx.clone();
    let target = destination.to_string();
    card.set_callback(move |event| {
        let _ = events.send((target.clone(), event));
    });
    let to = windows
        .get_mut(destination)
        .context("Destination surface disappeared")?;
    if let Some(previous) = to.cards.remove(id) {
        to.list.remove(&previous.widget, false);
    }
    to.snapshots.remove(id);
    to.states.remove(id);
    to.list.append(&card.widget);
    to.cards.insert(id.into(), card);
    Ok(())
}

/// Hyprland can report stale DnD-local coordinates after a layer/output focus
/// change. Read its logical cursor position directly, without spawning a process
/// per motion. Other compositors use the native drop coordinates instead.
fn compositor_cursor() -> Option<(f64, f64)> {
    use std::io::{Read, Write};
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    let signature = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let path = PathBuf::from(runtime)
        .join("hypr")
        .join(signature)
        .join(".socket.sock");
    let mut stream = std::os::unix::net::UnixStream::connect(path).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(20)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(20)))
        .ok()?;
    stream.write_all(b"j/cursorpos").ok()?;
    let mut bytes = Vec::new();
    stream.take(4096).read_to_end(&mut bytes).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    Some((value["x"].as_f64()?, value["y"].as_f64()?))
}

// Wayland delivers held modifiers only after the explicit click has acquired
// keyboard focus. Native DnD can defer that enter until its pointer grab ends.
fn finish_click(
    tx: mpsc::Sender<(String, CardEvent)>,
    source: String,
    id: String,
    control: bool,
    shift: bool,
    floating: bool,
) {
    glib::timeout_add_local_once(Duration::from_millis(60), move || {
        let modifiers = gdk::Display::default()
            .and_then(|display| display.default_seat())
            .and_then(|seat| seat.keyboard())
            .map(|keyboard| keyboard.modifier_state())
            .unwrap_or_else(gdk::ModifierType::empty);
        let intent = if shift || modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
            Intent::CopyMarkdown
        } else if control || modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
            Intent::ControlDismiss
        } else if !floating {
            Intent::Dismiss
        } else {
            return;
        };
        let _ = tx.send((source, CardEvent { id, intent }));
    });
}

fn popup(
    app: &gtk::Application,
    monitor: &gdk::Monitor,
    config: &Value,
    event_tx: &mpsc::Sender<(String, CardEvent)>,
    controller: Rc<RefCell<Controller>>,
    surface_name: &str,
    recording_id: Rc<RefCell<Option<String>>>,
    press: Rc<RefCell<Option<PointerPress>>>,
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
    list.set_widget_name(surface_name);
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
    let pointer_keyboard_until = Rc::new(std::cell::Cell::new(None));
    let modifiers = gtk::EventControllerKey::new();
    let modifier_press = press.clone();
    modifiers.connect_modifiers(move |_, modifiers| {
        if let Some(press) = modifier_press.borrow_mut().as_mut() {
            press.control |= modifiers.contains(gdk::ModifierType::CONTROL_MASK);
            press.shift |= modifiers.contains(gdk::ModifierType::SHIFT_MASK);
        }
        glib::Propagation::Proceed
    });
    window.add_controller(modifiers);
    let pointer = gtk::EventControllerLegacy::new();
    pointer.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = window.downgrade();
    let input = list.clone();
    let tx = event_tx.clone();
    let source = surface_name.to_string();
    let owner = controller.clone();
    // Native Wayland DnD owns the press through output changes. Layer-shell's
    // ordinary pointer grab is released by some compositors at output boundaries.
    let formats = gdk::ContentFormats::new(&["application/x-notjik-card"]);
    let target = gtk::DropTargetAsync::new(Some(formats), gdk::DragAction::MOVE);
    let current = press.clone();
    target.connect_accept(move |_, drop| {
        let accepted = current
            .borrow()
            .as_ref()
            .is_some_and(|p| p.native.as_ref() == drop.drag().as_ref());
        accepted
    });
    let current = press.clone();
    let target_window = window.downgrade();
    let target_tx = event_tx.clone();
    let target_owner = controller.clone();
    let update = Rc::new(move |x: f64, y: f64, finished: bool| {
        let Some(window) = target_window.upgrade() else {
            return;
        };
        let Some(monitor) = window.monitor() else {
            return;
        };
        let bounds = monitor.geometry();
        let (gx, gy) =
            compositor_cursor().unwrap_or((x + bounds.x() as f64, y + bounds.y() as f64));
        let mut state = current.borrow_mut();
        let Some(p) = state.as_mut() else { return };
        let (dx, dy) = (gx - p.x, gy - p.y);
        p.active |= dx.hypot(dy) >= 8.;
        let intent = if p.active {
            Some(Intent::Drag {
                dx,
                dy,
                finished,
                pointer: Some((gx - p.monitor_x, gy - p.monitor_y)),
            })
        } else {
            if finished {
                finish_click(
                    target_tx.clone(),
                    p.source.clone(),
                    p.id.clone(),
                    p.control,
                    p.shift,
                    target_owner.borrow().floating.contains(&p.id),
                );
            }
            None
        };
        if let Some(intent) = intent {
            let _ = target_tx.send((
                p.source.clone(),
                CardEvent {
                    id: p.id.clone(),
                    intent,
                },
            ));
        }
        if finished {
            state.take();
        }
    });
    let enter = update.clone();
    target.connect_drag_enter(move |_, _, x, y| {
        enter(x, y, false);
        gdk::DragAction::MOVE
    });
    let motion = update.clone();
    target.connect_drag_motion(move |_, _, x, y| {
        motion(x, y, false);
        gdk::DragAction::MOVE
    });
    target.connect_drop(move |_, drop, x, y| {
        update(x, y, true);
        drop.finish(gdk::DragAction::MOVE);
        true
    });
    window.add_controller(target);
    let click_until = pointer_keyboard_until.clone();
    pointer.connect_event(move |_, event| {
        if event.event_type() != gdk::EventType::ButtonPress
            || event
                .downcast_ref::<gdk::ButtonEvent>()
                .is_none_or(|b| b.button() != 1)
        {
            return glib::Propagation::Proceed;
        }
        let Some((x, y)) = event.position() else {
            return glib::Propagation::Proceed;
        };
        let Some(window) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        click_until.set(Some(Instant::now() + Duration::from_millis(350)));
        tjik_gtk::surface::keyboard(&window, true);
        if press.borrow().is_some() {
            return glib::Propagation::Stop;
        }
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
        let (Some(surface), Some(device), Some(monitor)) =
            (window.surface(), event.device(), window.monitor())
        else {
            return glib::Propagation::Proceed;
        };
        let bounds = monitor.geometry();
        let content = gdk::ContentProvider::for_bytes(
            "application/x-notjik-card",
            &glib::Bytes::from_static(b"notjik"),
        );
        let Some(drag) =
            gdk::Drag::begin(&surface, &device, &content, gdk::DragAction::MOVE, 0., 0.)
        else {
            return glib::Propagation::Proceed;
        };
        input.grab(true);
        *press.borrow_mut() = Some(PointerPress {
            id,
            source: source.clone(),
            x: x + bounds.x() as f64,
            y: y + bounds.y() as f64,
            active: false,
            control: event
                .modifier_state()
                .contains(gdk::ModifierType::CONTROL_MASK),
            shift: event
                .modifier_state()
                .contains(gdk::ModifierType::SHIFT_MASK),
            native: Some(drag.clone()),
            monitor_x: bounds.x() as f64,
            monitor_y: bounds.y() as f64,
        });
        // The original widget is rendered on the host's preview layer. Keep the
        // compositor drag icon transparent, avoiding a second visible card.
        let icon = gtk::DragIcon::for_drag(&drag);
        let blank = gtk::DrawingArea::new();
        blank.set_size_request(1, 1);
        icon.set_child(Some(&blank));
        let current = press.clone();
        let cancel_tx = tx.clone();
        let cancel_owner = owner.clone();
        drag.connect_cancel(move |drag, reason| {
            if !current
                .borrow()
                .as_ref()
                .is_some_and(|p| p.native.as_ref() == Some(drag))
            {
                drag.drop_done(false);
                return;
            }
            if let Some(p) = current.borrow_mut().take() {
                if !p.active && reason == gdk::DragCancelReason::NoTarget {
                    finish_click(
                        cancel_tx.clone(),
                        p.source,
                        p.id.clone(),
                        p.control,
                        p.shift,
                        cancel_owner.borrow().floating.contains(&p.id),
                    );
                } else {
                    let _ = cancel_tx.send((
                        p.source,
                        CardEvent {
                            id: p.id,
                            intent: Intent::CancelDrag,
                        },
                    ));
                }
            }
            drag.drop_done(false);
        });
        drag.connect_dnd_finished(|drag| drag.drop_done(true));
        glib::Propagation::Stop
    });
    window.add_controller(pointer);
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
    let keyboard_release_until = Rc::new(std::cell::Cell::new(None));
    let release_until = keyboard_release_until.clone();
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
            gdk::Key::Escape => {
                release_until.set(Some(Instant::now() + Duration::from_millis(100)));
                if let Some(window) = weak_window.upgrade() {
                    window.set_keyboard_mode(KeyboardMode::None);
                }
                json!({"op":"unfocus"})
            }
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
        native: false,
        window,
        list,
        scene,
        cards: HashMap::new(),
        snapshots: HashMap::new(),
        states: HashMap::new(),
        keyboard_selected: false,
        keyboard_was_active: false,
        pointer_keyboard_until,
        keyboard_release_until,
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
    #[ignore = "requires the current compositor; assigns workspace 7 before mapping"]
    fn native_detached_workspace_window() {
        gtk::init().unwrap();
        let app = gtk::Application::builder()
            .application_id("org.tjikos.notjik.nativefixture")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        let desktop = tjik_desktop::selected();
        let before = desktop.snapshot().unwrap();
        let workspace = before
            .workspaces
            .iter()
            .find(|w| w.workspace.name.as_deref() == Some("7"))
            .expect("workspace 7 exists");
        let output = before
            .outputs
            .iter()
            .find(|o| Some(&o.id) == workspace.output.as_ref())
            .unwrap();
        let monitor = gdk::Display::default()
            .unwrap()
            .monitors()
            .iter::<gdk::Monitor>()
            .flatten()
            .find(|m| m.connector().as_deref() == Some(&output.name))
            .unwrap();
        let title = format!("notjik-detached:fixture:{}", std::process::id());
        desktop
            .prepare_toplevel(
                &title,
                &tjik_desktop::WorkspaceTarget {
                    generation: before.generation.clone(),
                    output: output.id.clone(),
                    id: Some(workspace.workspace.id.clone()),
                    name: "7".into(),
                },
                tjik_desktop::Rect {
                    x: output.logical.x + 100.,
                    y: output.logical.y + 100.,
                    width: 420.,
                    height: 200.,
                },
            )
            .unwrap();
        struct Rule(String);
        impl Drop for Rule {
            fn drop(&mut self) {
                let _ = tjik_desktop::selected().release_toplevel(&self.0);
            }
        }
        let _rule = Rule(title.clone());
        let config = crate::config::defaults();
        let controller = Rc::new(RefCell::new(Controller::new(
            config.clone(),
            "native-fixture".into(),
        )));
        let (tx, rx) = mpsc::channel();
        let mut surface = popup(
            &app,
            &monitor,
            &config,
            &tx,
            controller,
            "fixture",
            Rc::new(RefCell::new(None)),
            Rc::new(RefCell::new(None)),
        );
        let card = Rc::new(Card::new(
            Notice(
                json!({"id":"native-fixture", "type":"telegram", "title":"Synthetic detached card", "body":"Workspace ownership", "data":{"demo":true,"kind":"text"}}),
            ),
            ViewState::default(),
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            now() as f64,
            |_| {},
        ));
        *card.context.draft.borrow_mut() = "Unsent fixture draft".into();
        surface.list.append(&card.widget);
        surface.cards.insert("native-fixture".into(), card.clone());
        surface.make_native(&app, &title, "fixture", "native-fixture", &tx);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mapped = loop {
            while glib::MainContext::default().pending() {
                glib::MainContext::default().iteration(false);
            }
            if let Some(window) = desktop
                .snapshot()
                .unwrap()
                .windows
                .into_iter()
                .find(|w| w.title.as_deref() == Some(&title))
            {
                break window;
            }
            assert!(Instant::now() < deadline, "Native notification did not map");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(
            mapped.workspace.as_ref().and_then(|w| w.name.as_deref()),
            Some("7")
        );
        assert_eq!(mapped.floating, Some(true));
        assert!(!surface.window.is_layer_window());
        assert!(!surface.window.is_active());
        assert!(Rc::ptr_eq(&surface.cards["native-fixture"], &card));
        assert_eq!(card.draft(), "Unsent fixture draft");
        surface.window.close();
        let (source, event) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(source, "fixture");
        assert_eq!(event.id, "native-fixture");
        assert!(matches!(event.intent, Intent::Dismiss));
        surface.window.destroy();
    }

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
    #[test]
    #[ignore = "requires the private compositor used by gtk_daemon.py"]
    fn host_lifecycle_preserves_drafts_and_identity() {
        gtk::init().unwrap();
        let app = gtk::Application::builder()
            .application_id("org.tjikos.notjik.lifecycle")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        let mut config = crate::config::defaults();
        config["placement"]["monitors"] = json!("all");
        config["grouping"]["enabled"] = json!(false);
        config["stack"]["animationMs"] = json!(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        config["style"] =
            serde_json::from_slice(&std::fs::read(root.join("themes/rose-punk.json")).unwrap())
                .unwrap();
        let mut controller = Controller::new(config, "lifecycle-fixture".into());
        controller.ready = true;
        controller.lock = "unlocked".into();
        let (event_tx, events) = mpsc::channel();
        let (_requests, requests) = mpsc::channel();
        let (_desktop, desktop) = mpsc::channel();
        let mut host = Host {
            showcase: None,
            root,
            app,
            controller: Rc::new(RefCell::new(controller)),
            bus: None,
            socket: None,
            requests,
            backend: None,
            pending: VecDeque::new(),
            events,
            event_tx,
            recording_id: Rc::new(RefCell::new(None)),
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
            monitor_tick: Instant::now(),
            desktop,
            desktop_events: None,
            workspaces: HashMap::new(),
            all_monitors: vec![],
            retained: HashMap::new(),
            pointer: Rc::new(RefCell::new(None)),
            hold: None,
            stopped: false,
        };
        fn frames() {
            let until = Instant::now() + Duration::from_millis(100);
            while Instant::now() < until {
                while glib::MainContext::default().pending() {
                    glib::MainContext::default().iteration(false);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        host.monitors();
        let source = host.windows.keys().next().unwrap().clone();
        let notice = json!({"id":"lifecycle", "type":"telegram", "title":"Synthetic lifecycle", "body":"Before replacement", "timeout":200, "data":{"kind":"text","demo":true,"chatId":"17","messageId":"23"}});
        let id = host
            .controller
            .borrow_mut()
            .send(notice.clone(), now())
            .unwrap();
        host.controller.borrow_mut().hold(&id, true);
        host.render();
        frames();
        host.controller.borrow_mut().selected = id.clone();
        host.render();
        host.controller
            .borrow_mut()
            .handle(json!({"op":"unfocus"}), now())
            .unwrap();
        host.render();
        assert_eq!(
            host.windows[&source].window.keyboard_mode(),
            KeyboardMode::None
        );
        frames();
        host.render();
        assert_eq!(
            host.windows[&source].window.keyboard_mode(),
            KeyboardMode::None
        );
        let card = host.windows[&source].cards[&id].clone();
        *card.context.draft.borrow_mut() = "Unsent synthetic reply".into();
        card.context.composing.set(true);
        card.context.quoted.set(true);
        *card.context.reply_target.borrow_mut() =
            Some(("17".into(), "23".into(), "Original quote".into()));
        card.render();
        frames();
        let quote = card.context.reply_target.borrow().clone();
        let check = |host: &Host| {
            let current = host
                .windows
                .values()
                .find_map(|s| s.cards.get(&id))
                .or_else(|| host.retained.get(&id))
                .unwrap();
            assert!(
                Rc::ptr_eq(current, &card),
                "Lifecycle rebuilt the card widget"
            );
            assert_eq!(current.draft(), "Unsent synthetic reply");
            assert_eq!(*current.context.reply_target.borrow(), quote);
            assert!(host.controller.borrow().find(&id).is_some());
        };
        for interruption in ["pause", "lock", "backend"] {
            let source = host
                .windows
                .iter()
                .find(|(_, s)| s.cards.contains_key(&id))
                .unwrap()
                .0
                .clone();
            host.event(
                &source,
                CardEvent {
                    id: id.clone(),
                    intent: Intent::Drag {
                        dx: -100.,
                        dy: -100.,
                        finished: false,
                        pointer: Some((300., 300.)),
                    },
                },
            )
            .unwrap();
            host.render();
            frames();
            assert!(host.drag.is_some());
            assert!(!host.windows[&source].cards.contains_key(&id));
            let mut replacement = notice.clone();
            replacement["body"] = json!("Replaced while dragging");
            host.controller
                .borrow_mut()
                .send(replacement, now())
                .unwrap();
            host.render();
            frames();
            check(&host);
            match interruption {
                "pause" => host.controller.borrow_mut().paused = true,
                "lock" => host.controller.borrow_mut().lock = "locked".into(),
                _ => {
                    let (client, service) = backend_channel::channel();
                    drop(service);
                    host.backend = Some(client);
                }
            }
            host.tick();
            frames();
            assert!(host.drag.is_none());
            assert!(host.windows.values().all(|s| !s.window.is_visible()));
            check(&host);
            host.controller.borrow_mut().tick(1000);
            assert!(host.controller.borrow().find(&id).is_some());
            host.backend = None;
            {
                let mut c = host.controller.borrow_mut();
                c.ready = true;
                c.paused = false;
                c.lock = "unlocked".into();
            }
            host.tick();
            frames();
            check(&host);
            host.controller.borrow_mut().tick(1000);
            assert!(
                host.controller.borrow().find(&id).is_some(),
                "Restored composer lost its expiry hold"
            );
        }
        // Telegram disconnection/uncertain send completion must retain the draft,
        // clear busy, and never enqueue an automatic retry.
        use std::io::BufRead;
        let bridge_dir = tempfile::tempdir().unwrap();
        let socket = bridge_dir.path().join("control.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let old_socket = std::env::var_os("NOTJIK_TELEGRAM_SOCKET");
        std::env::set_var("NOTJIK_TELEGRAM_SOCKET", &socket);
        listener.set_nonblocking(true).unwrap();
        let fake_bridge = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let stream = loop {
                if let Ok((stream, _)) = listener.accept() {
                    break stream;
                }
                assert!(
                    Instant::now() < deadline,
                    "Executor did not contact the fake bridge"
                );
                std::thread::sleep(Duration::from_millis(10));
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut line = String::new();
            std::io::BufReader::new(&stream)
                .read_line(&mut line)
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["action"], "reply");
            drop(stream); // Disconnect after receiving the mutation, before acknowledgement.
            listener
        });
        host.busy.insert(id.clone());
        host.executor.action(id.clone(), "reply".into(), json!({"action":"reply","text":"Synthetic private bridge test","chatId":"17","messageId":"23"}), false);
        let deadline = Instant::now() + Duration::from_secs(3);
        while host.busy.contains(&id) && Instant::now() < deadline {
            host.tick();
            frames();
        }
        let listener = fake_bridge.join().unwrap();
        if let Some(value) = old_socket {
            std::env::set_var("NOTJIK_TELEGRAM_SOCKET", value);
        } else {
            std::env::remove_var("NOTJIK_TELEGRAM_SOCKET");
        }
        check(&host);
        assert!(!host.busy.contains(&id));
        assert!(!host.feedback[&id].is_empty());
        frames();
        host.tick();
        listener.set_nonblocking(true).unwrap();
        assert!(
            listener.accept().is_err(),
            "Uncertain send was retried automatically"
        );
        host.controller.borrow_mut().tick(1000);
        assert!(host.controller.borrow().find(&id).is_some());
        // All outputs disappearing must preserve the same composer for reconnect.
        let removed = host.windows.keys().cloned().collect();
        host.retire_surfaces(removed, &HashSet::new());
        host.controller.borrow_mut().monitors.clear();
        host.controller.borrow_mut().tick(1000);
        check(&host);
        assert!(host.retained.contains_key(&id));
        host.monitors();
        host.render();
        frames();
        check(&host);
        assert!(!host.retained.contains_key(&id));
        host.controller.borrow_mut().close(&id, 2);
        host.render();
        assert!(host.windows.values().all(|s| !s.cards.contains_key(&id)));
        for surface in host.windows.values() {
            surface.window.close();
        }
        host.stop();
        println!("Lifecycle: replacement during drag, pause/resume, lock/unlock, backend loss/recovery, uncertain Telegram completion, all-output removal/reconnect, draft identity and expiry holds passed");
    }
}
