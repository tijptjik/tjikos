use crate::{
    clock_card, clocks, comparison, config, edges, layout, panel_view, panels, pointer,
    util::{arr, num, s},
    Action, Event,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    sync::Arc,
    time::{Duration, Instant},
};
use tjik_desktop::{Output, Rect, Snapshot};
use tjik_gtk::gtk::{self, gdk, glib, prelude::*};
fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
pub(super) fn surface(
    app: &gtk::Application,
    monitor: &gdk::Monitor,
    namespace: &str,
    layer: Layer,
) -> gtk::ApplicationWindow {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .decorated(false)
        .build();
    window.add_css_class("tjik");
    window.init_layer_shell();
    window.set_namespace(Some(namespace));
    window.set_monitor(Some(monitor));
    window.set_layer(layer);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Left, true);
    window.set_exclusive_zone(-1);
    window.set_keyboard_mode(KeyboardMode::None);
    // Never intercept desktop input between mapping and the first layout frame.
    window.connect_realize(|w| {
        if let Some(surface) = w.surface() {
            surface.set_input_region(&gtk::cairo::Region::create());
        }
    });
    window
}
fn position(window: &gtk::ApplicationWindow, r: Rect) {
    window.set_margin(Edge::Top, r.y.round() as i32);
    window.set_margin(Edge::Left, r.x.round() as i32);
    window.set_default_size(
        r.width.ceil().max(1.) as i32,
        r.height.ceil().max(1.) as i32,
    );
}
fn input_regions(window: &gtk::ApplicationWindow, islands: &[gtk::Box]) {
    let weak = window.downgrade();
    let islands: Vec<_> = islands.iter().map(|w| w.downgrade()).collect();
    let frames = std::cell::Cell::new(0);
    window.add_tick_callback(move |_, _| {
        let Some(window) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let region = gtk::cairo::Region::create();
        for island in islands.iter().filter_map(|w| w.upgrade()) {
            // A queued layout callback must not restore input after concealment.
            if window.opacity() <= 0.01 {
                break;
            }
            if let Some(r) = island.compute_bounds(&window) {
                let x = r.x().max(0.).floor() as i32;
                let y = r.y().max(0.).floor() as i32;
                let right = (r.x() + r.width()).min(window.width() as f32).ceil() as i32;
                let bottom = (r.y() + r.height()).min(window.height() as f32).ceil() as i32;
                if right > x && bottom > y {
                    let _ = region.union_rectangle(&gtk::cairo::RectangleInt::new(
                        x,
                        y,
                        right - x,
                        bottom - y,
                    ));
                }
            }
        }
        if let Some(surface) = window.surface() {
            surface.set_input_region(&region);
            surface.queue_render();
        }
        frames.set(frames.get() + 1);
        if frames.get() >= 3 {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}
pub(super) fn full_input(window: &gtk::ApplicationWindow) {
    let weak = window.downgrade();
    let frames = std::cell::Cell::new(0);
    window.add_tick_callback(move |_, _| {
        let Some(w) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if let Some(s) = w.surface() {
            s.set_input_region(&gtk::cairo::Region::create_rectangle(
                &gtk::cairo::RectangleInt::new(0, 0, w.width(), w.height()),
            ));
            s.queue_render();
        }
        frames.set(frames.get() + 1);
        if frames.get() >= 3 {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

struct Bar {
    window: gtk::ApplicationWindow,
    panel: gtk::ApplicationWindow,
    body: gtk::Box,
    rows: gtk::Box,
    islands: Vec<gtk::Box>,
    clocks: Vec<Rc<clock_card::Card>>,
    workspace_buttons: Vec<gtk::Button>,
    definition: Value,
    geometry: Rect,
    output: String,
    unsupported: Vec<String>,
    css: gtk::CssProvider,
    content: Value,
    resizing: bool,
    comparisons: Vec<(String, comparison::Module)>,
    hovered: Rc<Cell<bool>>,
    topology: Option<Output>,
    opacity: f64,
    target_opacity: f64,
    fade_from: f64,
    fade_started: Instant,
    input_active: bool,
}
impl Drop for Bar {
    fn drop(&mut self) {
        self.panel.destroy();
        self.window.destroy();
        if let Some(d) = gdk::Display::default() {
            gtk::style_context_remove_provider_for_display(&d, &self.css);
        }
    }
}
impl Bar {
    fn new(
        app: &gtk::Application,
        monitor: &gdk::Monitor,
        definition: &Value,
        send: &async_channel::Sender<Event>,
    ) -> Self {
        let layer = match s(&definition["layer"]) {
            "overlay" => Layer::Overlay,
            "bottom" => Layer::Bottom,
            "background" => Layer::Background,
            _ => Layer::Top,
        };
        let window = surface(app, monitor, "bartjik-gtk-bar", layer);
        let tracking = gtk::EventControllerMotion::new();
        let key = s(&definition["key"]).to_owned();
        let tx = send.clone();
        let k = key.clone();
        tracking.connect_enter(move |_, x, y| {
            let _ = tx.try_send(Event::Hover(k.clone(), Some((x, y))));
        });
        let tx = send.clone();
        let k = key.clone();
        tracking.connect_motion(move |_, x, y| {
            let _ = tx.try_send(Event::Hover(k.clone(), Some((x, y))));
        });
        let tx = send.clone();
        tracking.connect_leave(move |_| {
            let _ = tx.try_send(Event::Hover(key.clone(), None));
        });
        window.add_controller(tracking);
        let panel = surface(app, monitor, "bartjik-gtk-panel", Layer::Overlay);
        panel.set_keyboard_mode(KeyboardMode::OnDemand);
        panel.set_default_size(360, 240);
        let orientation = if layout::horizontal(definition) {
            gtk::Orientation::Horizontal
        } else {
            gtk::Orientation::Vertical
        };
        let body = gtk::Box::new(orientation, num(&definition["islandGap"]) as i32);
        let viewport = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::External)
            .vscrollbar_policy(gtk::PolicyType::External)
            .child(&body)
            .build();
        window.set_child(Some(&viewport));
        let pane = gtk::Box::new(gtk::Orientation::Vertical, 8);
        pane.add_css_class("pane");
        pane.append(&gtk::Label::new(Some("RUNNING WINDOWS")));
        let rows = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let scroll = gtk::ScrolledWindow::builder()
            .min_content_height(120)
            .max_content_height(420)
            .propagate_natural_height(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&rows)
            .build();
        pane.append(&scroll);
        let close = gtk::Button::with_label("Close");
        let target = panel.downgrade();
        close.connect_clicked(move |_| {
            if let Some(p) = target.upgrade() {
                p.set_visible(false);
            }
        });
        pane.append(&close);
        panel.set_child(Some(&pane));
        let keys = gtk::EventControllerKey::new();
        let target = panel.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(p) = target.upgrade() {
                    p.set_visible(false);
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        panel.add_controller(keys);
        panel.connect_close_request(|p| {
            p.set_visible(false);
            glib::Propagation::Stop
        });
        panel.connect_map(full_input);
        let hovered = Rc::new(Cell::new(false));
        let motion = gtk::EventControllerMotion::new();
        let entered = hovered.clone();
        motion.connect_enter(move |_, _, _| entered.set(true));
        let left = hovered.clone();
        motion.connect_leave(move |_| left.set(false));
        window.add_controller(motion);
        let css = tjik_gtk::theme::provider();
        Self {
            window,
            panel,
            body,
            rows,
            islands: vec![],
            clocks: vec![],
            workspace_buttons: vec![],
            definition: definition.clone(),
            geometry: Rect {
                x: 0.,
                y: 0.,
                width: 0.,
                height: 0.,
            },
            output: s(&definition["screen"]).into(),
            unsupported: vec![],
            css,
            content: Value::Null,
            resizing: false,
            comparisons: vec![],
            hovered,
            topology: None,
            opacity: 1.,
            target_opacity: 1.,
            fade_from: 1.,
            fade_started: Instant::now(),
            input_active: true,
        }
    }
    fn refresh(
        &mut self,
        snapshot: &Snapshot,
        output: &Output,
        actions: &async_channel::Sender<Action>,
        _demo: bool,
        can_select: bool,
        sender: &async_channel::Sender<Event>,
    ) {
        self.topology = Some(output.clone());
        // Focus-only push events must not replace a button during its press/release.
        let content = json!({"generation":snapshot.generation,"output":output,"modules":self.definition["moduleConfigs"],
            "workspaces":snapshot.workspaces,
            "windows":snapshot.windows.iter().map(|w|json!({"id":w.id,"title":w.title,
                "app_id":w.app_id,"output":w.output,"workspace":w.workspace,"mapped":w.mapped,"hidden":w.hidden})).collect::<Vec<_>>()});
        if self.content == content {
            return;
        }
        self.content = content;
        if !self.comparisons.is_empty() {
            for (id, view) in &mut self.comparisons {
                view.update(&self.definition["moduleConfigs"][id.as_str()]);
            }
            self.relayout();
            return;
        }
        clear(&self.body);
        clear(&self.rows);
        self.islands.clear();
        let mut previous_clocks = std::mem::take(&mut self.clocks);
        self.workspace_buttons.clear();
        self.unsupported.clear();
        let horizontal = layout::horizontal(&self.definition);
        let orientation = if horizontal {
            gtk::Orientation::Horizontal
        } else {
            gtk::Orientation::Vertical
        };
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let class = format!("bartjik-{}", NEXT.fetch_add(1, Ordering::Relaxed));
        // Replace the previous scope rather than accumulate providers or global rules.
        for old in self.window.css_classes() {
            if old.starts_with("bartjik-") {
                self.window.remove_css_class(&old);
            }
        }
        self.window.add_css_class(&class);
        let color = |v: &Value| {
            gdk::RGBA::parse(s(v))
                .unwrap_or(gdk::RGBA::BLACK)
                .to_string()
        };
        let mut css=format!(".{class} button {{min-width:16px;min-height:20px;padding:0 3px;border:0;background:transparent;box-shadow:none;}} .{class} .workspace-active {{color:{};}} .{class} button:hover {{background:alpha({},0.12);}}",color(&self.definition["style"]["accent"]),color(&self.definition["style"]["accent"]));
        for (index, definition) in arr(&self.definition["islands"]).iter().enumerate() {
            if arr(&definition["modules"]).is_empty() {
                continue;
            }
            let style = config::merge(&self.definition["style"], &definition["style"]);
            let island = gtk::Box::new(orientation, num(&style["moduleGap"]) as i32);
            island.add_css_class(&format!("island-{index}"));
            let px = style["paddingX"].as_f64().unwrap_or(num(&style["padding"]));
            let py = style["paddingY"].as_f64().unwrap_or(num(&style["padding"]));
            css+=&format!(".{class} .island-{index} {{background:{};color:{};border:{}px solid {};border-radius:{}px;padding:{}px {}px;}}",color(&style["background"]),color(&style["text"]),num(&style["borderWidth"]),color(&style["border"]),num(&style["radius"]),py,px);
            if horizontal {
                island.set_size_request(-1, num(&self.definition["thickness"]) as i32);
            } else {
                island.set_size_request(num(&self.definition["verticalWidth"]) as i32, -1);
            }
            for id in arr(&definition["modules"]) {
                let module = &self.definition["moduleConfigs"][s(id)];
                if module["comparisonItems"].is_array() {
                    let view = comparison::Module::new(module, &style, orientation, sender.clone());
                    island.append(&view.root);
                    self.comparisons.push((s(id).into(), view));
                    continue;
                }
                match s(&module["provider"]) {
                    "workspaces" => {
                        let group = gtk::Box::new(orientation, num(&style["itemGap"]) as i32);
                        for row in layout::workspaces(snapshot, output, &self.output, module) {
                            let button = gtk::Button::with_label(&row.label);
                            button
                                .set_tooltip_text(Some(&format!("Workspace {}", row.target.name)));
                            button.set_sensitive(can_select);
                            if row.active {
                                button.add_css_class("workspace-active");
                            }
                            let send = actions.clone();
                            button.connect_clicked(move |_| {
                                let _ = send.try_send(Action::Workspace(row.target.clone()));
                            });
                            group.append(&button);
                            self.workspace_buttons.push(button);
                        }
                        let open = gtk::Button::with_label("▦");
                        open.set_tooltip_text(Some("Running windows"));
                        let target = self.panel.downgrade();
                        open.connect_clicked(move |_| {
                            if let Some(p) = target.upgrade() {
                                p.set_visible(!p.is_visible());
                            }
                        });
                        group.append(&open);
                        island.append(&group);
                    }
                    "datetime" => {
                        let card = if let Some(index) =
                            previous_clocks.iter().position(|c| c.module == *module)
                        {
                            previous_clocks.remove(index)
                        } else {
                            clock_card::Card::new(
                                &self.window.application().unwrap(),
                                &self.window.monitor().unwrap(),
                                module,
                            )
                        };
                        for old in card.panel.css_classes() {
                            if old.starts_with("bartjik-") {
                                card.panel.remove_css_class(&old);
                            }
                        }
                        card.panel.add_css_class(&class);
                        island.append(&card.button);
                        self.clocks.push(card);
                    }
                    provider => {
                        let label = gtk::Label::new(Some(&format!("{provider} · unavailable")));
                        label.set_tooltip_text(Some(
                            "This provider has not been migrated to GTK yet",
                        ));
                        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                        label.set_max_width_chars(22);
                        island.append(&label);
                        self.unsupported.push(provider.into());
                    }
                }
            }
            self.body.append(&island);
            self.islands.push(island);
        }
        css += &format!(".{class} .clock-row {{background:{};border-radius:8px;padding:10px 12px;min-height:36px;}} .{class} .clock-city {{font-size:13px;}} .{class} .clock-detail {{font-size:10px;color:{};}} .{class} .clock-value {{font-size:23px;}} .{class} .clock-heading {{font-size:16px;}}", color(&self.definition["style"]["surface"]),color(&self.definition["style"]["muted"]));
        self.css.load_from_string(&css);

        for window in snapshot.windows.iter().filter(|w| {
            w.output.as_ref() == Some(&output.id)
                && w.mapped != Some(false)
                && w.hidden != Some(true)
        }) {
            let button = gtk::Button::with_label(
                window
                    .title
                    .as_deref()
                    .or(window.app_id.as_deref())
                    .unwrap_or("Window"),
            );
            if let Some(l) = button.child().and_downcast::<gtk::Label>() {
                l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                l.set_max_width_chars(40);
            }
            let id = window.id.clone();
            let send = actions.clone();
            let panel = self.panel.downgrade();
            button.connect_clicked(move |_| {
                if send.try_send(Action::Window(id.clone())).is_ok() {
                    if let Some(p) = panel.upgrade() {
                        p.set_visible(false);
                    }
                }
            });
            self.rows.append(&button);
        }
        if self.rows.first_child().is_none() {
            self.rows
                .append(&gtk::Label::new(Some("No windows on this output")));
        }
        self.relayout();
    }
    fn relayout(&mut self) {
        let Some(output) = self.topology.clone() else {
            return;
        };
        let width = self.body.measure(gtk::Orientation::Horizontal, -1).1 as f64;
        let height = self
            .body
            .measure(gtk::Orientation::Vertical, width as i32)
            .1 as f64;
        self.geometry = layout::geometry(
            &self.definition,
            output.logical.width,
            output.logical.height,
            width,
            height,
            output.scale,
        );
        position(&self.window, self.geometry);
        let visible = self.definition["visible"] != false
            && self.geometry.width > 0.
            && self.geometry.height > 0.;
        self.window.set_visible(visible);
        if visible && self.input_active {
            input_regions(&self.window, &self.islands);
        } else if !visible {
            self.panel.set_visible(false);
        }
        let r = self.geometry;
        let panel_height = 240_f64.min(output.logical.height);
        let panel_width = 360_f64.min(output.logical.width);
        let (x, y) = match s(&self.definition["side"]) {
            "b" => (r.x, (r.y - panel_height - 8.).max(0.)),
            "l" => (r.x + r.width + 8., r.y),
            "r" => ((r.x - panel_width - 8.).max(0.), r.y),
            _ => (r.x, r.y + r.height + 8.),
        };
        position(
            &self.panel,
            Rect {
                x: x.min((output.logical.width - panel_width).max(0.)).max(0.),
                y: y.min((output.logical.height - panel_height).max(0.))
                    .max(0.),
                width: panel_width,
                height: panel_height,
            },
        );
        for clock in &self.clocks {
            let width = 440_f64.min(output.logical.width);
            let height = (160.
                + 58.
                    * (if clock.module["worldClocks"].is_null() {
                        4
                    } else {
                        arr(&clock.module["worldClocks"]).len().clamp(1, 7)
                    }) as f64)
                .min(output.logical.height);
            let bounds = clock.button.compute_bounds(&self.window);
            let center_x = r.x
                + bounds
                    .as_ref()
                    .map(|v| (v.x() + v.width() / 2.) as f64)
                    .unwrap_or(r.width / 2.);
            let center_y = r.y
                + bounds
                    .as_ref()
                    .map(|v| (v.y() + v.height() / 2.) as f64)
                    .unwrap_or(r.height / 2.);
            let (x, y) = match s(&self.definition["side"]) {
                "b" => (center_x - width / 2., r.y - height - 8.),
                "l" => (r.x + r.width + 8., center_y - height / 2.),
                "r" => (r.x - width - 8., center_y - height / 2.),
                _ => (center_x - width / 2., r.y + r.height + 8.),
            };
            position(
                &clock.panel,
                Rect {
                    x: x.clamp(0., (output.logical.width - width).max(0.)),
                    y: y.clamp(0., (output.logical.height - height).max(0.)),
                    width,
                    height,
                },
            );
            if !visible {
                clock.hide();
            }
        }
    }
    fn conceal(&mut self, concealed: bool) {
        let target = if concealed { 0. } else { 1. };
        if target != self.target_opacity {
            self.fade_from = self.opacity;
            self.target_opacity = target;
            self.fade_started = Instant::now();
        }
        if !concealed {
            for (_, view) in &self.comparisons {
                view.wake_icons();
                for visual in view.visuals() {
                    visual.wake();
                }
            }
        }
        let duration = num(&self.definition["fadeDuration"]);
        let t = if duration <= 0. {
            1.
        } else {
            (self.fade_started.elapsed().as_secs_f64() * 1000. / duration).min(1.)
        };
        let eased = if t < 0.5 {
            2. * t * t
        } else {
            1. - (-2. * t + 2.).powi(2) / 2.
        };
        self.opacity = self.fade_from + (self.target_opacity - self.fade_from) * eased;
        if (self.window.opacity() - self.opacity).abs() > 0.0001 {
            self.window.set_opacity(self.opacity);
        }
        let active = self.opacity > 0.01;
        if active != self.input_active {
            self.input_active = active;
            if active {
                input_regions(&self.window, &self.islands);
            } else if let Some(surface) = self.window.surface() {
                surface.set_input_region(&gtk::cairo::Region::create());
                surface.queue_render();
                self.hovered.set(false);
            }
        }
    }
    fn clock(&mut self, readings: &BTreeMap<String, clocks::Reading>) {
        let mut changed = false;
        for clock in &self.clocks {
            changed |= clock.update(readings.get(&clock.key));
        }
        if changed {
            self.relayout();
        }
    }
}
struct Reservation(gtk::ApplicationWindow);
impl Drop for Reservation {
    fn drop(&mut self) {
        self.0.destroy();
    }
}
pub struct Ui {
    pub app: gtk::Application,
    pub snapshot: RefCell<Option<Snapshot>>,
    bars: RefCell<BTreeMap<String, Bar>>,
    reservations: RefCell<BTreeMap<String, Reservation>>,
    configuration: RefCell<Value>,
    comparison: RefCell<Value>,
    error: RefCell<String>,
    actions: async_channel::Sender<Action>,
    demo: bool,
    reserve: bool,
    workspace_scope: Option<String>,
    can_select: bool,
    runtime: Option<crate::runtime::Runtime>,
    sender: async_channel::Sender<Event>,
    backend: RefCell<Value>,
    activity: RefCell<Value>,
    alternate: RefCell<std::collections::HashSet<String>>,
    intents: RefCell<Vec<Value>>,
    menu: RefCell<Option<crate::tray_menu::Menu>>,
    pointer: pointer::Pointer,
    deck: RefCell<panels::Deck>,
    panel_surface: RefCell<Option<panel_view::Surface>>,
    audio_card: Rc<crate::audio_card::Card>,
    history_card: Rc<crate::history_card::Card>,
    left_cards: BTreeMap<String, Rc<crate::left_card::Card>>,
    system_cards: BTreeMap<String, Rc<crate::system_card::Card>>,
    network_card: Rc<crate::network_card::Card>,
    connectivity_card: Rc<crate::connectivity_card::Card>,
    local_pointer: Cell<Option<(f64, f64)>>,
    local_updated: RefCell<Instant>,
    fixture_pointer: Cell<Option<(f64, f64)>>,
    clocks: clocks::Service,
    edge_states: RefCell<BTreeMap<String, edges::State>>,
    edge_timer: RefCell<Option<glib::SourceId>>,
    started: Instant,
}
impl Ui {
    pub fn new(
        app: gtk::Application,
        configuration: Value,
        actions: async_channel::Sender<Action>,
        demo: bool,
        reserve: bool,
        can_select: bool,
        desktop: Arc<dyn tjik_desktop::Desktop>,
        runtime: Option<crate::runtime::Runtime>,
        sender: async_channel::Sender<Event>,
    ) -> Self {
        Self {
            app,
            snapshot: RefCell::new(None),
            bars: RefCell::new(BTreeMap::new()),
            reservations: RefCell::new(BTreeMap::new()),
            configuration: RefCell::new(configuration),
            comparison: RefCell::new(Value::Null),
            error: RefCell::new(String::new()),
            actions: actions.clone(),
            demo,
            reserve,
            workspace_scope: std::env::var("BARTJIK_GTK_WORKSPACE")
                .ok()
                .filter(|v| !v.is_empty()),
            can_select,
            left_cards: ["crypto", "playerctl", "workspaces"]
                .into_iter()
                .map(|p| {
                    (p.into(), {
                        let card = crate::left_card::Card::new(p, sender.clone(), actions.clone());
                        if p == "workspaces" {
                            card.capture_with(desktop.clone());
                        }
                        card
                    })
                })
                .collect(),
            system_cards: ["caffeine", "nightlight", "monitor-layout"]
                .into_iter()
                .map(|p| (p.into(), crate::system_card::Card::new(p, sender.clone())))
                .collect(),
            connectivity_card: crate::connectivity_card::Card::new(sender.clone()),
            history_card: crate::history_card::Card::new(demo || runtime.is_none(), sender.clone()),
            audio_card: crate::audio_card::Card::new(demo || runtime.is_none(), sender.clone()),
            network_card: crate::network_card::Card::new(
                if demo || runtime.is_none() {
                    Arc::new(crate::network::Fake::demo())
                } else {
                    crate::network::selected()
                },
                sender.clone(),
            ),
            runtime,
            sender,
            backend: RefCell::new(Value::Null),
            activity: RefCell::new(json!({"hyprvoice":"unavailable","screenRecording":false})),
            alternate: RefCell::new(Default::default()),
            intents: RefCell::new(vec![]),
            menu: RefCell::new(None),
            pointer: pointer::Pointer::new(desktop),
            deck: RefCell::new(Default::default()),
            panel_surface: RefCell::new(None),
            local_pointer: Cell::new(None),
            local_updated: RefCell::new(Instant::now()),
            fixture_pointer: Cell::new(None),
            clocks: clocks::Service::new(demo),
            edge_states: RefCell::new(BTreeMap::new()),
            edge_timer: RefCell::new(None),
            started: Instant::now(),
        }
    }
    pub fn clock(&self) {
        let readings = self.clocks.readings();
        for b in self.bars.borrow_mut().values_mut() {
            b.clock(&readings);
        }
    }
    fn inspect(
        widgets: impl Iterator<Item = gtk::Widget>,
        window: &gtk::ApplicationWindow,
    ) -> Vec<Value> {
        widgets.filter_map(|w|{let r=w.compute_bounds(window)?;let label=w.downcast_ref::<gtk::Button>().and_then(|b|b.label()).map(|s|s.to_string());Some(json!({"label":label,"x":r.x(),"y":r.y(),"width":r.width(),"height":r.height()}))}).collect()
    }
    fn status(&self) -> Value {
        json!({"workspaceScope":self.workspace_scope,"workspaceOutputs":self.snapshot.borrow().as_ref().map(|s|s.outputs.iter().filter(|o|layout::workspace_visible(o,self.workspace_scope.as_deref())).map(|o|o.name.clone()).collect::<Vec<_>>()),"panelDeck":self.panel_status(),"ok":true,"pid":std::process::id(),"nativeRuntime":self.runtime.is_some(),"backendReady":self.backend.borrow()["ready"],"intents":*self.intents.borrow(),"demo":self.demo,"reserve":self.reserve,"autoHideError":self.pointer.latest().1,"pointerSampling":self.edge_timer.borrow().is_some(),"clockProviders":self.clocks.count(),"comparison":std::env::var_os("BARTJIK_GTK_COMPARE_FILE").is_some(),"error":self.error.borrow().as_str(),"revision":self.snapshot.borrow().as_ref().map(|s|s.revision),"bars":self.bars.borrow().iter().map(|(key,b)|{
            let mut children=vec![];let mut child=b.rows.first_child();while let Some(w)=child {child=w.next_sibling();if w.is::<gtk::Button>(){children.push(w);}}
            json!({"key":key,"output":b.output,"side":b.definition["side"],"visible":b.window.is_visible(),"concealed":self.edge_states.borrow().get(key).is_some_and(|s|s.concealed),"opacity":b.opacity,"inputActive":b.input_active,"geometry":b.geometry,"width":b.window.width(),"height":b.window.height(),"unsupported":b.unsupported,"panel":b.panel.is_visible(),"items":Self::inspect(children.into_iter(),&b.panel),"clocks":b.clocks.iter().map(|c|{let mut v=c.status();v["bounds"]=Self::inspect(std::iter::once(c.button.clone().upcast()),&b.window).first().cloned().unwrap_or(Value::Null);v}).collect::<Vec<_>>(),"workspaces":Self::inspect(b.workspace_buttons.iter().map(|w|w.clone().upcast()),&b.window),"motion":b.comparisons.iter().map(|(id,v)|json!({"id":id,"visible":v.root.is_visible(),"icons":v.icon_status(),"tray":v.tray_status(),"visuals":v.visuals().iter().map(|v|v.status()).collect::<Vec<_>>()})).collect::<Vec<_>>(),"islands":Self::inspect(b.islands.iter().map(|w|w.clone().upcast()),&b.window)})
        }).collect::<Vec<_>>()})
    }
    pub fn render(self: &Rc<Self>, snapshot: &Snapshot) {
        let display = gdk::Display::default().unwrap();
        let monitors: Vec<_> = display
            .monitors()
            .iter::<gdk::Monitor>()
            .flatten()
            .collect();
        let outputs: Vec<_> = monitors
            .iter()
            .filter_map(|m| {
                let name = m.connector()?.to_string();
                let mut output = if self.demo {
                    snapshot.outputs.first()?.clone()
                } else {
                    snapshot.outputs.iter().find(|o| o.name == name)?.clone()
                };
                if self.demo {
                    let g = m.geometry();
                    output.name = name;
                    output.logical.x = g.x() as f64;
                    output.logical.y = g.y() as f64;
                    output.logical.width = g.width() as f64;
                    output.logical.height = g.height() as f64;
                    output.scale = m.scale_factor() as f64;
                }
                Some(output)
            })
            .collect();
        let screens=json!(outputs.iter().map(|o|json!({"name":o.name,"width":o.logical.width,"height":o.logical.height,"scale":o.scale})).collect::<Vec<_>>());
        if let Some(runtime) = &self.runtime {
            runtime.screens(screens.clone());
        }
        let definitions = match config::resolve_all(&self.configuration.borrow(), &screens) {
            Ok(v) => v,
            Err(e) => {
                *self.error.borrow_mut() = e.to_string();
                return;
            }
        };
        let mut live = vec![];
        let mut depths = BTreeMap::<(String, String), i32>::new();
        for mut definition in definitions {
            {
                let mut live_rows = vec![];
                if self.runtime.is_some() {
                    let backend = self.backend.borrow();
                    let resolved = arr(&backend["bars"])
                        .iter()
                        .find(|b| b["key"] == definition["key"]);
                    if let Some(modules) = definition["moduleConfigs"].as_object() {
                        for (name, module) in modules {
                            let items = if module["provider"] == "workspaces" {
                                outputs.iter().find(|o|o.name==s(&definition["screen"])).map(|o|json!(layout::workspaces(snapshot,o,&o.name,module).iter().map(|r|json!({"id":r.target.name,"text":{"value":r.label},"type":"text","target":r.target,"special":r.target.name.starts_with("special:"),"state":{"active":r.active},"actions":{"leftClick":"workspace.activate","scrollUp":"workspace.previous","scrollDown":"workspace.next"}})).collect::<Vec<_>>())).unwrap_or(json!([]))
                            } else {
                                resolved
                                    .map(|b| {
                                        backend["providers"][s(&b["sources"][name])]["items"]
                                            .clone()
                                    })
                                    .filter(Value::is_array)
                                    .unwrap_or(json!([]))
                            };
                            live_rows.push(json!({"id":name,"bar":definition["id"],"monitor":definition["screen"],"items":items,"activity":*self.activity.borrow()}));
                        }
                    }
                }
                let rows = if self.runtime.is_some() {
                    json!(live_rows)
                } else {
                    self.comparison.borrow().clone()
                };
                let id = s(&definition["id"]).to_owned();
                let output = s(&definition["screen"]).to_owned();
                let context_key = definition["key"].clone();
                let islands = definition["islands"].clone();
                let configured = definition["moduleConfigs"].clone();
                if let Some(modules) = definition["moduleConfigs"].as_object_mut() {
                    for (name, module) in modules {
                        let peer = |provider: &str| {
                            arr(&islands)
                                .iter()
                                .find(|island| arr(&island["modules"]).iter().any(|v| v == name))
                                .and_then(|island| {
                                    arr(&island["modules"]).iter().find(|v| {
                                        s(v) != name && configured[s(v)]["provider"] == provider
                                    })
                                })
                                .map(|v| s(v).to_owned())
                        };
                        let mirrored = |key: &str| {
                            rows.as_array().and_then(|rs| {
                                rs.iter().find(|r| {
                                    r["bar"] == id && r["monitor"] == output && r["id"] == key
                                })
                            })
                        };
                        module["inputContext"] =
                            json!({"barKey":context_key,"bar":id,"monitor":output,"module":name});
                        module["comparisonActivity"] = mirrored(name)
                            .map(|r| r["activity"].clone())
                            .unwrap_or(Value::Null);
                        module["comparisonHidden"] =
                            json!(module["provider"] == "tailscale" && peer("mullvad").is_some());
                        module["comparisonTailnet"] = json!(
                            module["provider"] == "mullvad"
                                && peer("tailscale")
                                    .and_then(|p| mirrored(&p))
                                    .is_some_and(|r| r["items"][0]["connection"] == "Running"
                                        || r["items"][0]["panel"]["connection"] == "Running")
                        );
                        if module["provider"] == "mullvad" {
                            if let Some(peer_name) = peer("tailscale") {
                                let mut peer_module = configured[&peer_name].clone();
                                peer_module["inputContext"] = json!({"barKey":context_key,"bar":id,"monitor":output,"module":peer_name});
                                if let Some(peer_row) =
                                    mirrored(&peer_name).and_then(|r| arr(&r["items"]).first())
                                {
                                    module["comparisonTailnetInput"] =
                                        crate::input::payload(&peer_module, peer_row, "icon");
                                }
                            }
                        }
                        module["comparisonItems"] = rows
                            .as_array()
                            .and_then(|rs| {
                                rs.iter().find(|r| {
                                    r["bar"] == id && r["monitor"] == output && r["id"] == *name
                                })
                            })
                            .map(|r| r["items"].clone())
                            .unwrap_or_else(|| json!([]));
                        if let Some(items) = module["comparisonItems"].as_array_mut() {
                            for row in items {
                                let key = format!("{}/{}/{}", s(&context_key), name, s(&row["id"]));
                                if self.alternate.borrow().contains(&key)
                                    && row["alternate"].is_string()
                                {
                                    row["text"]["value"] = row["alternate"].clone();
                                }
                            }
                        }
                    }
                }
            }
            let Some(output) = outputs.iter().find(|o| o.name == s(&definition["screen"])) else {
                continue;
            };
            if !layout::workspace_visible(output, self.workspace_scope.as_deref()) {
                continue;
            }
            let Some(monitor) = monitors
                .iter()
                .find(|m| m.connector().as_deref() == Some(&output.name))
            else {
                continue;
            };
            let key = s(&definition["key"]).to_owned();
            live.push(key.clone());
            let mut bars = self.bars.borrow_mut();
            let structural = |value: &Value| {
                let mut value = value.clone();
                if let Some(modules) = value["moduleConfigs"].as_object_mut() {
                    for module in modules.values_mut() {
                        if let Some(m) = module.as_object_mut() {
                            m.retain(|key, _| {
                                !key.starts_with("comparison") && key != "inputContext"
                            });
                        }
                    }
                }
                value
            };
            if bars
                .get(&key)
                .is_some_and(|b| structural(&b.definition) != structural(&definition))
            {
                if self
                    .menu
                    .borrow()
                    .as_ref()
                    .is_some_and(|m| m.context["barKey"] == key)
                {
                    self.menu.borrow_mut().take();
                }
                bars.remove(&key);
            }
            let bar = bars
                .entry(key.clone())
                .or_insert_with(|| Bar::new(&self.app, monitor, &definition, &self.sender));
            bar.definition = definition.clone();
            bar.refresh(
                snapshot,
                output,
                &self.actions,
                self.demo,
                self.can_select,
                &self.sender,
            );
            if !bar.resizing && bar.comparisons.iter().any(|(_, v)| v.resizing()) {
                bar.resizing = true;
                let weak = Rc::downgrade(self);
                let key = key.clone();
                bar.window.add_tick_callback(move |_, _| {
                    let Some(ui) = weak.upgrade() else {
                        return glib::ControlFlow::Break;
                    };
                    let mut bars = ui.bars.borrow_mut();
                    let Some(bar) = bars.get_mut(&key) else {
                        return glib::ControlFlow::Break;
                    };
                    for (_, view) in &bar.comparisons {
                        view.resize_frame();
                    }
                    bar.relayout();
                    let moving = bar.comparisons.iter().any(|(_, v)| v.resizing());
                    if moving {
                        glib::ControlFlow::Continue
                    } else {
                        bar.resizing = false;
                        glib::ControlFlow::Break
                    }
                });
            }
            if self.reserve && bar.window.is_visible() {
                let depth = layout::reserve(
                    &definition,
                    bar.geometry,
                    output.logical.width,
                    output.logical.height,
                );
                let d = depths
                    .entry((output.name.clone(), s(&definition["side"]).into()))
                    .or_default();
                *d = (*d).max(depth);
            }
        }
        if self
            .menu
            .borrow()
            .as_ref()
            .is_some_and(|m| !live.iter().any(|key| m.context["barKey"] == *key))
        {
            self.menu.borrow_mut().take();
        }
        self.bars.borrow_mut().retain(|k, _| live.contains(k));
        if self.bars.borrow().is_empty() {
            self.local_pointer.set(None);
        }
        let mut reservations = self.reservations.borrow_mut();
        let mut live = vec![];
        for ((name, side), depth) in depths {
            if depth <= 0 {
                continue;
            }
            let Some(monitor) = monitors
                .iter()
                .find(|m| m.connector().as_deref() == Some(&name))
            else {
                continue;
            };
            let key = format!("{name}/{side}");
            live.push(key.clone());
            let reservation = reservations.entry(key.clone()).or_insert_with(|| {
                let w = surface(&self.app, monitor, "bartjik-gtk-reservation", Layer::Top);
                for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                    w.set_anchor(edge, false);
                }
                let edges = match side.as_str() {
                    "t" => vec![Edge::Top, Edge::Left, Edge::Right],
                    "b" => vec![Edge::Bottom, Edge::Left, Edge::Right],
                    "l" => vec![Edge::Left, Edge::Top, Edge::Bottom],
                    _ => vec![Edge::Right, Edge::Top, Edge::Bottom],
                };
                for e in edges {
                    w.set_anchor(e, true);
                }
                w.set_default_size(1, 1);
                Reservation(w)
            });
            reservation.0.set_exclusive_zone(depth);
            reservation.0.set_visible(true);
        }
        reservations.retain(|k, _| live.contains(k));
        drop(reservations);
        self.clocks.configure(
            self.bars
                .borrow()
                .values()
                .filter(|b| b.window.is_visible())
                .flat_map(|b| b.clocks.iter().map(|c| c.module.clone())),
        );
        self.clock();
        self.update_panels();
        self.configure_edges();
    }
    fn panel_targets(&self) -> Vec<panels::Target> {
        let mut targets = vec![];
        for (key, bar) in self
            .bars
            .borrow()
            .iter()
            .filter(|(_, b)| b.window.is_visible() && b.input_active)
        {
            let Some(output) = &bar.topology else {
                continue;
            };
            let offset = (
                output.logical.x + bar.geometry.x,
                output.logical.y + bar.geometry.y,
            );
            for (id, view) in &bar.comparisons {
                let module = &bar.definition["moduleConfigs"][id];
                let provider = s(&module["provider"]);
                let Some(island) = view
                    .root
                    .parent()
                    .and_then(|w| w.compute_bounds(&bar.window))
                else {
                    continue;
                };
                let source = Rect {
                    x: offset.0 + island.x() as f64,
                    y: offset.1 + island.y() as f64,
                    width: island.width() as f64,
                    height: island.height() as f64,
                };
                for (item, mut bounds) in view.hit_regions(bar.window.upcast_ref()) {
                    let Some(row) = arr(&module["comparisonItems"])
                        .iter()
                        .find(|r| r["id"] == item)
                    else {
                        continue;
                    };
                    bounds.x += offset.0;
                    bounds.y += offset.1;
                    let options = if provider == "workspaces" {
                        &module["preview"]
                    } else {
                        &module["panel"]
                    };
                    let implicit = [
                        "audio",
                        "network",
                        "nightlight",
                        "monitor-layout",
                        "caffeine",
                    ]
                    .contains(&provider)
                        && s(&crate::action_choice::choose(
                            module,
                            row,
                            "item",
                            "hoverEnter",
                        ))
                        .ends_with(".panel");
                    let enabled = row["enabled"] != false
                        && (row["idle"] != true
                            || (provider == "playerctl" && row["native"].is_object()))
                        && if provider == "workspaces" {
                            options["enabled"] == true
                        } else {
                            options["hover"] == true || (implicit && options["hover"] != false)
                        };
                    targets.push(panels::Target {
                        key: format!("{key}/{id}/{item}"),
                        bar: key.clone(),
                        module: id.clone(),
                        item,
                        output: bar.output.clone(),
                        side: s(&bar.definition["side"]).into(),
                        bounds,
                        anchor: None,
                        source,
                        bar_bounds: Rect {
                            x: offset.0,
                            y: offset.1,
                            width: bar.geometry.width,
                            height: bar.geometry.height,
                        },
                        screen: output.logical,
                        options: options.clone(),
                        row: row.clone(),
                        provider: provider.into(),
                        style: config::merge(&bar.definition["style"], &module["style"]),
                        enabled,
                    });
                }
            }
        }
        targets
    }
    fn panel_status(&self) -> Value {
        let deck = self.deck.borrow();
        json!({"network":self.network_card.diagnostics(),"active":deck.active.as_ref().map(|t|&t.key),"pinned":deck.pinned,"pending":deck.pending_key(),"surface":self.panel_surface.borrow().as_ref().map(|p|p.status()),"targets":self.panel_targets().iter().map(|t|json!({"key":t.key,"bar":t.bar,"module":t.module,"item":t.item,"output":t.output,"side":t.side,"bounds":t.bounds,"source":t.source,"screen":t.screen,"enabled":t.enabled,"delay":t.options["delay"]})).collect::<Vec<_>>()})
    }
    fn update_panels(&self) {
        // Mirror comparison is presentation-only; it never opens native panels.
        if self.runtime.is_none() {
            return;
        }
        let targets = self.panel_targets();
        let recent = self.local_updated.borrow().elapsed() < Duration::from_millis(100);
        let point = self.fixture_pointer.get().or(if recent {
            self.local_pointer.get().or(self.pointer.latest().0)
        } else {
            self.pointer.latest().0.or_else(|| {
                self.bars
                    .borrow()
                    .values()
                    .any(|b| b.hovered.get())
                    .then(|| self.local_pointer.get())
                    .flatten()
            })
        });
        let hit = point.and_then(|p| panels::nearest(&targets, p));
        let (rect, inside) = self
            .panel_surface
            .borrow()
            .as_ref()
            .map(|p| (Some(p.rect()), p.inside.get()))
            .unwrap_or((None, false));
        {
            let mut deck = self.deck.borrow_mut();
            if let Some(active) = deck.active.as_ref() {
                if let Some(current) = targets.iter().find(|t| t.key == active.key) {
                    let mut current = current.clone();
                    current.anchor = active.anchor;
                    deck.active = Some(current);
                } else {
                    deck.close();
                }
            }
            deck.step(
                hit,
                point,
                rect,
                inside,
                self.started.elapsed().as_millis() as u64,
            );
        }
        self.sync_panel();
    }

    fn sync_panel(&self) {
        let (target, pinned) = {
            let deck = self.deck.borrow();
            (deck.active.clone(), deck.pinned)
        };
        let Some(target) = target else {
            self.panel_surface.borrow_mut().take();
            return;
        };
        let monitor = gdk::Display::default()
            .unwrap()
            .monitors()
            .iter::<gdk::Monitor>()
            .flatten()
            .find(|m| m.connector().as_deref() == Some(&target.output));
        let Some(monitor) = monitor else {
            self.deck.borrow_mut().close();
            self.panel_surface.borrow_mut().take();
            return;
        };
        if let Some(card) = self.system_cards.get(&target.provider) {
            card.update(
                target.row["panel"].clone(),
                json!({"barKey":target.bar,"module":target.module,"monitor":target.output}),
                target.style.clone(),
            );
        }
        let vpn = ["mullvad", "tailscale"].contains(&target.provider.as_str());
        if vpn {
            let bars = self.bars.borrow();
            if let Some(bar) = bars.get(&target.bar) {
                let mut ids = vec![target.module.clone()];
                if target.provider == "mullvad" {
                    if let Some(island) = arr(&bar.definition["islands"])
                        .iter()
                        .find(|i| arr(&i["modules"]).iter().any(|id| id == &target.module))
                    {
                        if let Some(id) = arr(&island["modules"]).iter().find(|id| {
                            bar.definition["moduleConfigs"][s(id)]["provider"] == "tailscale"
                        }) {
                            ids.push(s(id).into());
                        }
                    }
                }
                let sections: Vec<_> = ids.iter().map(|id| {
                    let module = &bar.definition["moduleConfigs"][id];
                    json!({"provider":module["provider"],"row":arr(&module["comparisonItems"]).first().cloned().unwrap_or(json!({})),"context":{"barKey":target.bar,"module":id,"monitor":target.output}})
                }).collect();
                self.connectivity_card.update(json!({"sections":sections}));
            }
        }
        let mut panel = self.panel_surface.borrow_mut();
        if panel.as_ref().is_some_and(|p| p.output != target.output) {
            panel.take();
        }
        panel
            .get_or_insert_with(|| {
                panel_view::Surface::new(&self.app, &monitor, &target.output, self.sender.clone())
            })
            .show(&target, pinned, || {
                if let Some(card) = self.left_cards.get(&target.provider) {
            card.update(target.row.clone(), json!({"barKey":target.bar,"module":target.module,"item":target.item,"monitor":target.output,"maxWindows":target.options["maxWindows"],"previewWidth":target.options["width"]}), target.style.clone(), self.snapshot.borrow().clone());
                    card.content()
                } else if let Some(card) = self.system_cards.get(&target.provider) {
                    card.content()
                } else if target.provider == "DND" {
                    self.history_card.content()
                } else if target.provider == "locale" {
                    panel_view::locale_content(&target, self.sender.clone())
                } else if target.provider == "audio" {
                    self.audio_card.content(&target.style)
                } else if target.provider == "network" {
                    self.network_card.content()
                } else if vpn {
                    self.connectivity_card.content()
                } else {
                    panel_view::status_content(&target)
                }
            });
        if let Some(card) = self.left_cards.get(&target.provider) {
            card.update(target.row.clone(), json!({"barKey":target.bar,"module":target.module,"item":target.item,"monitor":target.output,"maxWindows":target.options["maxWindows"],"previewWidth":target.options["width"]}), target.style.clone(), self.snapshot.borrow().clone());
        }
    }

    fn tracking_needed(&self) -> bool {
        self.bars.borrow().values().any(|b| {
            b.window.is_visible()
                && (b.definition["autoHide"] == true || (self.runtime.is_some() && b.hovered.get()))
        }) || (self.local_pointer.get().is_some()
            && self.local_updated.borrow().elapsed() < Duration::from_millis(100))
            || (self.deck.borrow().busy() && !self.deck.borrow().pinned)
            || self.fixture_pointer.get().is_some()
    }
    fn configure_edges(self: &Rc<Self>) {
        let enabled = self.tracking_needed();
        self.pointer.enable(enabled);
        if enabled && self.edge_timer.borrow().is_none() {
            let weak = Rc::downgrade(self);
            *self.edge_timer.borrow_mut() = Some(glib::timeout_add_local(
                Duration::from_millis(25),
                move || {
                    let Some(ui) = weak.upgrade() else {
                        return glib::ControlFlow::Break;
                    };
                    ui.update_panels();
                    ui.update_edges();
                    if !ui.tracking_needed() {
                        ui.pointer.enable(false);
                        ui.edge_timer.borrow_mut().take();
                        glib::ControlFlow::Break
                    } else {
                        glib::ControlFlow::Continue
                    }
                },
            ));
        } else if !enabled {
            if let Some(timer) = self.edge_timer.borrow_mut().take() {
                timer.remove();
            }
        }
        self.update_edges();
    }
    fn update_edges(&self) {
        let (pointer, _) = self.pointer.latest();
        let mut bars = self.bars.borrow_mut();
        let entries: Vec<_> = bars
            .iter()
            .filter(|(_, b)| b.window.is_visible())
            .filter_map(|(key, b)| {
                Some(edges::Entry {
                    key,
                    definition: &b.definition,
                    output: b.topology.as_ref()?,
                    hovered: self
                        .deck
                        .borrow()
                        .active
                        .as_ref()
                        .is_some_and(|t| t.bar == *key)
                        || b.hovered.get()
                        || b.panel.is_visible()
                        || b.clocks.iter().any(|c| c.panel.is_visible()),
                    fading: b.opacity > 0.01,
                })
            })
            .collect();
        let states = edges::update(
            &self.edge_states.borrow(),
            &entries,
            pointer,
            self.started.elapsed().as_millis() as u64,
        );
        for (key, bar) in bars.iter_mut() {
            bar.conceal(states.get(key).is_some_and(|s| s.concealed));
        }
        // Match legacy reservation policy: concealed bars release their edge.
        for (key, reservation) in self.reservations.borrow().iter() {
            let depth = bars
                .iter()
                .filter(|(bar_key, b)| {
                    format!("{}/{}", b.output, s(&b.definition["side"])) == *key
                        && b.window.is_visible()
                        && !states.get(*bar_key).is_some_and(|s| s.concealed)
                })
                .filter_map(|(_, b)| {
                    b.topology.as_ref().map(|o| {
                        layout::reserve(
                            &b.definition,
                            b.geometry,
                            o.logical.width,
                            o.logical.height,
                        )
                    })
                })
                .max()
                .unwrap_or(0);
            reservation.0.set_exclusive_zone(depth);
            reservation.0.set_visible(depth > 0);
        }
        *self.edge_states.borrow_mut() = states;
    }
    fn input(self: &Rc<Self>, mut context: Value) {
        let selected = {
            let bars = self.bars.borrow();
            let Some(bar) = bars.get(s(&context["barKey"])) else {
                return;
            };
            let module = &bar.definition["moduleConfigs"][s(&context["module"])];
            let Some(row) = arr(&module["comparisonItems"])
                .iter()
                .find(|r| r["id"] == context["item"])
            else {
                return;
            };
            if row["enabled"] == false {
                return;
            }
            let action = crate::action_choice::choose(
                module,
                row,
                s(&context["part"]),
                s(&context["event"]),
            );
            let local = &context["localAnchor"];
            context["anchor"] = json!({"x":bar.geometry.x + local["x"].as_f64().unwrap_or(0.),"y":bar.geometry.y + local["y"].as_f64().unwrap_or(0.),"width":local["width"].as_f64().unwrap_or(bar.geometry.width),"height":local["height"].as_f64().unwrap_or(bar.geometry.height)});
            (module.clone(), row.clone(), action)
        };
        let (module, row, action) = selected;
        if action.is_null() {
            return;
        }
        if action == "datetime.toggle" {
            let key = format!(
                "{}/{}/{}",
                s(&context["barKey"]),
                s(&context["module"]),
                s(&context["item"])
            );
            let mut alt = self.alternate.borrow_mut();
            if !alt.remove(&key) {
                alt.insert(key);
            }
            drop(alt);
            if let Some(snapshot) = self.snapshot.borrow().as_ref() {
                self.render(snapshot);
            }
            return;
        }
        let menu_path = s(&row["native"]["props"]["Menu"]);
        if (action == "tray.menu"
            || (action == "tray.activate" && row["native"]["props"]["ItemIsMenu"] == true))
            && menu_path.starts_with('/')
            && menu_path != "/NO_DBUSMENU"
            && menu_path != "/"
        {
            if let Some(runtime) = &self.runtime {
                let _ = runtime.send(json!({"op":"nativeMenu","context":context,"menuId":0}));
            }
            return;
        }
        if action == "tray.activate" {
            let app = s(&row["native"]["props"]["Id"]).to_lowercase();
            let classes: Vec<&str> = match app.as_str() {
                "telegramdesktop" => vec![
                    "org.telegram.desktop",
                    "telegramdesktop",
                    "telegram-desktop",
                ],
                "zapzap" => vec!["com.rtosta.zapzap", "zapzap"],
                "chatgpt" => vec!["chatgpt"],
                "codex" => vec!["codex", "com.openai.codex"],
                _ => vec![],
            };
            if let Some(window) = self.snapshot.borrow().as_ref().and_then(|snap| {
                snap.windows
                    .iter()
                    .find(|w| {
                        w.mapped != Some(false)
                            && w.app_id
                                .as_ref()
                                .is_some_and(|a| classes.iter().any(|c| a.eq_ignore_ascii_case(c)))
                    })
                    .cloned()
            }) {
                let _ = self.actions.try_send(Action::Window(window.id));
                return;
            }
        }
        if s(&action).ends_with(".panel") {
            if s(&context["event"]).starts_with("hover") {
                return;
            }
            if let Some(target) = self.panel_targets().into_iter().find(|t| {
                t.bar == s(&context["barKey"])
                    && t.module == s(&context["module"])
                    && t.item == s(&context["item"])
            }) {
                self.deck.borrow_mut().open(target);
                self.update_panels();
                self.configure_edges();
            }
            return;
        }
        if self.runtime.is_none() {
            *self.error.borrow_mut() =
                "Read-only comparison; use native GTK mode for actions".into();
            return;
        }
        if s(&action).starts_with("workspace.") {
            let rows: Vec<_> = arr(&module["comparisonItems"])
                .iter()
                .filter(|r| r["special"] != true || module["scrollSpecial"] == true)
                .collect();
            let target = if action == "workspace.activate" {
                &row
            } else {
                let Some(index) = rows.iter().position(|r| r["state"]["active"] == true) else {
                    return;
                };
                let offset = if action == "workspace.next" {
                    1
                } else {
                    rows.len() - 1
                };
                rows[(index + offset) % rows.len()]
            };
            if let Ok(target) = serde_json::from_value(target["target"].clone()) {
                let _ = self.actions.try_send(Action::Workspace(target));
            }
            return;
        }
        if let Some(runtime) = &self.runtime {
            if let Err(e) = runtime.send(json!({"op":"action","context":context})) {
                *self.error.borrow_mut() = e;
            }
        }
    }
    pub fn event(self: &Rc<Self>, event: Event) {
        match event {
            Event::Hover(key, point) => {
                *self.local_updated.borrow_mut() = Instant::now();
                if self.runtime.is_none() {
                    return;
                }
                if let Some(point) = point {
                    if let Some(bar) = self.bars.borrow().get(&key) {
                        if let Some(output) = &bar.topology {
                            self.local_pointer.set(Some((
                                output.logical.x + bar.geometry.x + point.0,
                                output.logical.y + bar.geometry.y + point.1,
                            )));
                        }
                    }
                } else {
                    self.local_pointer.set(None);
                }
                self.update_panels();
                self.configure_edges();
            }
            Event::PanelCommand(command) => {
                if command == "resize" {
                    self.sync_panel();
                    return;
                } else if command == "detach" {
                    self.deck.borrow_mut().pinned = true;
                } else if command == "pin" {
                    let mut deck = self.deck.borrow_mut();
                    deck.pinned = !deck.pinned;
                } else {
                    self.deck.borrow_mut().close();
                }
                self.sync_panel();
                self.configure_edges();
            }
            Event::LeftRequest(request) => {
                if self.demo
                    && matches!(s(&request["op"]), "action" | "mediaAction" | "cryptoRetry")
                {
                    self.intents
                        .borrow_mut()
                        .push(json!({"kind":"intent","request":request}));
                } else if let Some(runtime) = &self.runtime {
                    if let Err(error) = runtime.send(request) {
                        *self.error.borrow_mut() = error;
                    }
                }
            }
            Event::SystemAction(provider, request) => {
                if let Some(card) = self.system_cards.get(&provider) {
                    if self.demo {
                        self.intents
                            .borrow_mut()
                            .push(json!({"kind":"intent","request":request}));
                        card.finish("Demo action recorded", false);
                    } else if let Some(runtime) = &self.runtime {
                        if let Err(error) = runtime.send(request) {
                            card.finish(&error, true);
                        }
                    } else {
                        card.finish("Backend unavailable", true);
                    }
                }
            }
            Event::ConnectivityAction(context) => {
                if self.demo {
                    self.intents
                        .borrow_mut()
                        .push(json!({"kind":"intent","context":context}));
                    self.connectivity_card.finish("Demo action recorded");
                } else if let Some(runtime) = &self.runtime {
                    if let Err(error) = runtime.send(json!({"op":"panelAction","context":context}))
                    {
                        self.connectivity_card.finish(&error);
                    }
                } else {
                    self.connectivity_card.finish("Backend unavailable");
                }
            }
            Event::Input(context) => self.input(context),
            Event::Backend(value) => {
                if matches!(
                    s(&value["kind"]),
                    "crypto" | "artwork" | "lyrics" | "mediaPosition"
                ) {
                    for card in self.left_cards.values() {
                        card.receive(&value);
                    }
                    return;
                }
                if value["kind"] == "menuChanged" {
                    if let (Some(menu), Some(runtime)) =
                        (self.menu.borrow().as_ref(), self.runtime.as_ref())
                    {
                        if menu.popover.is_visible()
                            && value["owner"] == menu.owner
                            && value["path"] == menu.path
                        {
                            let _ = runtime.send(
                                json!({"op":"nativeMenu","context":menu.context,"menuId":menu.id}),
                            );
                        }
                    }
                    return;
                }
                if value["kind"] == "menu" {
                    if let Some(error) = value["error"].as_str() {
                        *self.error.borrow_mut() = error.into();
                        self.menu.borrow_mut().take();
                        return;
                    }
                    if value["activated"] == true {
                        self.menu.borrow_mut().take();
                        return;
                    }
                    if let (Some(bar), Some(runtime)) = (
                        self.bars.borrow().get(s(&value["context"]["barKey"])),
                        self.runtime.as_ref(),
                    ) {
                        self.menu.borrow_mut().take();
                        let menu = crate::tray_menu::Menu::new(
                            bar.body.upcast_ref(),
                            &value,
                            runtime.clone(),
                        );
                        *self.menu.borrow_mut() = Some(menu);
                    }
                    return;
                }
                if value["kind"] == "intent" {
                    let mut log = self.intents.borrow_mut();
                    log.push(value);
                    if log.len() > 64 {
                        log.remove(0);
                    }
                    return;
                }
                if !value["ready"].is_null() {
                    self.connectivity_card.acknowledge(&value["panelFeedback"]);
                    for card in self.system_cards.values() {
                        card.acknowledge(&value["panelFeedback"]);
                        card.monitor_change(value["monitorChange"].clone());
                    }
                    if value["config"].is_object() {
                        *self.configuration.borrow_mut() = value["config"].clone();
                    }
                    if let Some(error) = value["actionError"].as_str().filter(|e| !e.is_empty()) {
                        *self.error.borrow_mut() = error.into();
                    }
                    let menu_removed = self.menu.borrow().as_ref().is_some_and(|menu| {
                        !value["providers"].as_object().is_some_and(|providers| {
                            providers.values().any(|provider| {
                                arr(&provider["items"]).iter().any(|row| {
                                    row["native"]["owner"] == menu.owner
                                        && row["native"]["props"]["Menu"] == menu.path
                                })
                            })
                        })
                    });
                    if menu_removed {
                        self.menu.borrow_mut().take();
                    }
                    *self.backend.borrow_mut() = value;
                    if let Some(snapshot) = self.snapshot.borrow().as_ref() {
                        self.render(snapshot);
                    }
                }
            }
            Event::Activity(kind, value) => {
                if kind == "hyprvoice-status" {
                    self.activity.borrow_mut()["hyprvoice"] = if value["available"] == true {
                        value["state"].clone()
                    } else {
                        json!("unavailable")
                    };
                } else {
                    self.activity.borrow_mut()["screenRecording"] =
                        json!(value["available"] == true && value["recording"] == true);
                }
                if let Some(snapshot) = self.snapshot.borrow().as_ref() {
                    self.render(snapshot);
                }
            }
            Event::Quit => {
                self.menu.borrow_mut().take();
                self.deck.borrow_mut().close();
                self.panel_surface.borrow_mut().take();
                self.clocks.stop();
                self.app.quit();
            }
            Event::Control(value, reply) => {
                match value["action"].as_str().unwrap_or("") {
                    "panel" | "hide-panel" => {
                        if value["action"] == "hide-panel" {
                            self.deck.borrow_mut().close();
                            self.sync_panel();
                            self.configure_edges();
                        }
                        for b in self.bars.borrow().values() {
                            b.panel
                                .set_visible(value["action"] == "panel" && b.window.is_visible());
                            if value["action"] == "hide-panel" {
                                for c in &b.clocks {
                                    c.hide();
                                }
                            }
                        }
                    }
                    "reload" => match config::load() {
                        Ok(c) => {
                            *self.configuration.borrow_mut() = c;
                            if let Some(s) = self.snapshot.borrow().as_ref() {
                                self.render(s);
                            }
                        }
                        Err(e) => {
                            let _ = reply.send(json!({"ok":false,"error":e.to_string()}));
                            return;
                        }
                    },
                    "input" if self.demo => {
                        self.input(value["context"].clone());
                    }
                    "panel-command" if self.demo => {
                        self.event(Event::PanelCommand(s(&value["command"]).into()));
                    }
                    "pointer" if self.demo => {
                        self.fixture_pointer.set(
                            value["point"]
                                .as_array()
                                .and_then(|v| Some((v.first()?.as_f64()?, v.get(1)?.as_f64()?))),
                        );
                        self.update_panels();
                        self.configure_edges();
                    }
                    "panel-close" => {
                        self.deck.borrow_mut().close();
                        self.sync_panel();
                        self.configure_edges();
                    }
                    "status" => (),
                    _ => {
                        let _ = reply.send(json!({"ok":false,"error":"Unknown action"}));
                        return;
                    }
                }
                let _ = reply.send(self.status());
            }
            Event::Comparison(value) => {
                *self.comparison.borrow_mut() = value;
                if let Some(snapshot) = self.snapshot.borrow().as_ref() {
                    self.render(snapshot);
                }
            }
            Event::Result(message) => {
                for b in self.bars.borrow().values() {
                    b.window.set_tooltip_text(if message.is_empty() {
                        None
                    } else {
                        Some(&message)
                    });
                }
                *self.error.borrow_mut() = message;
            }
            Event::Desktop(tjik_desktop::DesktopState::Unavailable(error)) => {
                *self.error.borrow_mut() = error.to_string();
                self.menu.borrow_mut().take();
                self.deck.borrow_mut().close();
                self.panel_surface.borrow_mut().take();
                self.bars.borrow_mut().clear();
                self.reservations.borrow_mut().clear();
                self.snapshot.borrow_mut().take();
                self.clocks.configure(std::iter::empty());
                self.configure_edges();
            }
            Event::Desktop(tjik_desktop::DesktopState::Ready(snapshot)) => {
                self.error.borrow_mut().clear();
                *self.snapshot.borrow_mut() = Some(snapshot.clone());
                self.render(&snapshot);
            }
        }
    }
}
