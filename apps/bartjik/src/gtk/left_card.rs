//! Left-side panels. Requests use existing core services; windows use Desktop IDs.
use crate::{
    panel_view::Content,
    util::{arr, num, s},
    Action, Event,
};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};
use tjik_desktop::Snapshot;
use tjik_gtk::gtk::{self, gdk, prelude::*};
fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.);
    l.set_wrap(true);
    l.add_css_class(class);
    l
}
fn button(text: &str, name: &str) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.set_widget_name(name);
    b.add_css_class("policy-button");
    b
}
// GTK's automatic tooltip placement can cover the bar below the overflow strip.
// An ungrabbing popover gives these window titles an explicit top anchor.
pub(super) fn overflow_tooltip(button: &gtk::Button, title: &str) -> gtk::Popover {
    button.set_tooltip_text(Some(title));
    button.set_has_tooltip(false);
    let popup = gtk::Popover::new();
    popup.add_css_class("workspace-tooltip");
    popup.set_position(gtk::PositionType::Top);
    popup.set_autohide(false);
    popup.set_has_arrow(false);
    popup.set_can_focus(false);
    popup.set_can_target(false);
    let text = label(title, "workspace-tooltip-title");
    text.set_max_width_chars(36);
    // The one-pixel CSS border participates in GTK layout.
    text.set_margin_top(7);
    text.set_margin_bottom(7);
    text.set_margin_start(9);
    text.set_margin_end(9);
    popup.set_child(Some(&text));
    let pending = Rc::new(RefCell::new(None::<gtk::glib::SourceId>));
    let motion = gtk::EventControllerMotion::new();
    let timer = pending.clone();
    let weak = button.downgrade();
    let tip = popup.clone();
    motion.connect_enter(move |_, _, _| {
        if let Some(source) = timer.borrow_mut().take() {
            source.remove();
        }
        let weak = weak.clone();
        let tip = tip.clone();
        let timer_done = timer.clone();
        *timer.borrow_mut() = Some(gtk::glib::timeout_add_local_once(
            std::time::Duration::from_millis(650),
            move || {
                timer_done.borrow_mut().take();
                if let Some(button) = weak.upgrade().filter(|b| b.is_mapped()) {
                    if tip.parent().is_none() {
                        tip.set_parent(&button);
                    }
                    tip.popup();
                }
            },
        ));
    });
    let timer = pending.clone();
    let tip = popup.clone();
    motion.connect_leave(move |_| {
        if let Some(source) = timer.borrow_mut().take() {
            source.remove();
        }
        tip.popdown();
        if tip.parent().is_some() {
            tip.unparent();
        }
    });
    button.add_controller(motion);
    let tip = popup.clone();
    button.connect_unmap(move |_| {
        if let Some(source) = pending.borrow_mut().take() {
            source.remove();
        }
        tip.popdown();
        if tip.parent().is_some() {
            tip.unparent();
        }
    });
    popup
}
fn music_button(name: &str, transport: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_widget_name(name);
    button.add_css_class("music-button");
    if transport {
        button.add_css_class("music-transport");
    }
    button.set_has_frame(false);
    button.set_focus_on_click(false);
    button.set_valign(gtk::Align::Center);
    button.set_halign(gtk::Align::Center);
    button.set_size_request(
        if transport { 42 } else { 32 },
        if transport { 40 } else { 32 },
    );
    button
}
fn application_icon(app: &str) -> gtk::Image {
    thread_local! {
        static APPS: Vec<gio::AppInfo> = gio::AppInfo::all();
    }
    let icon = APPS.with(|apps| {
        apps.iter()
            .find(|a| {
                a.id()
                    .is_some_and(|id| id.trim_end_matches(".desktop").eq_ignore_ascii_case(app))
                    || (*a)
                        .clone()
                        .downcast::<gio::DesktopAppInfo>()
                        .ok()
                        .and_then(|d| d.startup_wm_class())
                        .is_some_and(|class| class.eq_ignore_ascii_case(app))
            })
            .and_then(|a| a.icon())
    });
    if let Some(icon) = icon {
        gtk::Image::from_gicon(&icon)
    } else {
        let theme = gtk::IconTheme::for_display(&gdk::Display::default().unwrap());
        gtk::Image::from_icon_name(if theme.has_icon(app) {
            app
        } else {
            "application-x-executable"
        })
    }
}
fn spotify_id(meta: &Value) -> String {
    for key in ["xesam:url", "mpris:trackid"] {
        let raw = s(&meta[key]);
        for prefix in [
            "spotify:track:",
            "/com/spotify/track/",
            "https://open.spotify.com/track/",
        ] {
            if let Some(id) = raw
                .strip_prefix(prefix)
                .map(|v| v.split(['?', '#']).next().unwrap_or(""))
            {
                if id.len() == 22 && id.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return id.into();
                }
            }
        }
    }
    String::new()
}
pub struct Card {
    captures: RefCell<Option<async_channel::Sender<(String, tjik_desktop::WindowId, u32, u32)>>>,
    thumbnails: RefCell<Vec<(tjik_desktop::WindowId, gtk::Picture)>>,
    capture_pending: Cell<bool>,
    workspace_active: Cell<bool>,
    capture_retry: Cell<std::time::Instant>,
    capture_index: Cell<usize>,
    frames: RefCell<BTreeMap<String, gdk::MemoryTexture>>,
    provider: &'static str,
    body: gtk::Box,
    row: RefCell<Value>,
    context: RefCell<Value>,
    style: RefCell<Value>,
    snapshot: RefCell<Option<Snapshot>>,
    chart: RefCell<Value>,
    chart_trace: Rc<RefCell<crate::crypto_chart::Trace>>,
    art: RefCell<String>,
    lyrics: RefCell<Value>,
    show_lyrics: Cell<bool>,
    sequence: Cell<u64>,
    tokens: RefCell<BTreeMap<String, String>>,
    send: async_channel::Sender<Event>,
    actions: async_channel::Sender<Action>,
    key: RefCell<String>,
    clock: RefCell<(std::time::Instant, f64, f64)>,
    clock_poll: RefCell<std::time::Instant>,
    lyric_rows: RefCell<Vec<(f64, gtk::Label)>>,
    lyric_scroll: RefCell<Option<gtk::ScrolledWindow>>,
    follow_after: RefCell<std::time::Instant>,
    active_line: Cell<Option<usize>>,
    ticker: RefCell<Option<gtk::glib::SourceId>>,
}
impl Card {
    pub fn new(
        provider: &'static str,
        send: async_channel::Sender<Event>,
        actions: async_channel::Sender<Action>,
    ) -> Rc<Self> {
        let c = Rc::new(Self {
            captures: RefCell::new(None),
            thumbnails: RefCell::new(Vec::new()),
            capture_pending: Cell::new(false),
            workspace_active: Cell::new(true),
            capture_retry: Cell::new(std::time::Instant::now()),
            capture_index: Cell::new(0),
            frames: RefCell::new(BTreeMap::new()),
            provider,
            body: gtk::Box::new(gtk::Orientation::Vertical, 12),
            row: RefCell::new(Value::Null),
            context: RefCell::new(Value::Null),
            style: RefCell::new(Value::Null),
            snapshot: RefCell::new(None),
            chart: RefCell::new(Value::Null),
            chart_trace: Rc::new(RefCell::new(crate::crypto_chart::Trace::default())),
            art: RefCell::new(String::new()),
            lyrics: RefCell::new(Value::Null),
            show_lyrics: Cell::new(false),
            sequence: Cell::new(0),
            tokens: RefCell::new(BTreeMap::new()),
            send,
            actions,
            key: RefCell::new(String::new()),
            clock: RefCell::new((std::time::Instant::now(), 0., 0.)),
            clock_poll: RefCell::new(std::time::Instant::now() - std::time::Duration::from_secs(2)),
            lyric_rows: RefCell::new(Vec::new()),
            lyric_scroll: RefCell::new(None),
            follow_after: RefCell::new(std::time::Instant::now()),
            active_line: Cell::new(None),
            ticker: RefCell::new(None),
        });
        c.body.add_css_class("left-card");
        let weak = Rc::downgrade(&c);
        c.body.connect_map(move |_| {
            if let Some(c) = weak.upgrade() {
                if c.provider == "crypto"
                    && num(&c.chart.borrow()["updated"]) > 0.
                    && chrono::Local::now().timestamp() as f64 - num(&c.chart.borrow()["updated"])
                        > 60.
                {
                    *c.chart.borrow_mut() = Value::Null;
                }
                c.load();
                c.start_ticker();
            }
        });
        let weak = Rc::downgrade(&c);
        c.body.connect_unmap(move |_| {
            if let Some(c) = weak.upgrade() {
                c.tokens.borrow_mut().clear();
                c.frames.borrow_mut().clear();
                for (_, picture) in c.thumbnails.borrow().iter() {
                    picture.set_paintable(gdk::Paintable::NONE);
                }
                if let Some(ticker) = c.ticker.borrow_mut().take() {
                    ticker.remove();
                }
            }
        });
        c
    }
    fn start_ticker(self: &Rc<Self>) {
        if self.provider == "playerctl" && !self.show_lyrics.get() {
            if let Some(ticker) = self.ticker.borrow_mut().take() {
                ticker.remove();
            }
            return;
        }
        if self.provider == "crypto" || self.ticker.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(self);
        let ticker =
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                let Some(c) = weak.upgrade() else {
                    return gtk::glib::ControlFlow::Break;
                };
                if !c.workspace_active.get() {
                    return gtk::glib::ControlFlow::Continue;
                }
                if c.provider == "workspaces"
                    && c.body.is_mapped()
                    && !c.capture_pending.get()
                    && std::time::Instant::now() >= c.capture_retry.get()
                {
                    let thumbs = c.thumbnails.borrow();
                    if !thumbs.is_empty() {
                        let i = c.capture_index.get() % thumbs.len();
                        c.capture_index.set(i + 1);
                        if let Some(tx) = c.captures.borrow().as_ref() {
                            let picture = &thumbs[i].1;
                            if picture.width() <= 0 || picture.height() <= 0 {
                                return gtk::glib::ControlFlow::Continue;
                            }
                            let scale = picture
                                .native()
                                .and_then(|n| n.surface())
                                .map_or(picture.scale_factor() as f64, |s| s.scale());
                            let width = (picture.width() as f64 * scale).round().max(1.) as u32;
                            let height = (picture.height() as f64 * scale).round().max(1.) as u32;
                            if tx
                                .try_send((
                                    c.key.borrow().clone(),
                                    thumbs[i].0.clone(),
                                    width,
                                    height,
                                ))
                                .is_ok()
                            {
                                c.capture_pending.set(true);
                            }
                        }
                    }
                }
                if c.provider == "playerctl" && c.body.is_mapped() && c.show_lyrics.get() {
                    if c.clock_poll.borrow().elapsed().as_secs_f64() >= 1. {
                        *c.clock_poll.borrow_mut() = std::time::Instant::now();
                        c.request("mediaPosition", json!({"native":c.row.borrow()["native"]}));
                    }
                    c.highlight();
                }
                gtk::glib::ControlFlow::Continue
            });
        *self.ticker.borrow_mut() = Some(ticker);
    }
    pub fn set_workspace_active(&self, active: bool) {
        self.workspace_active.set(active);
    }
    pub fn capture_with(self: &Rc<Self>, desktop: std::sync::Arc<dyn tjik_desktop::Desktop>) {
        let (tx, rx) = async_channel::bounded::<(String, tjik_desktop::WindowId, u32, u32)>(1);
        let (results, incoming) = async_channel::bounded(1);
        *self.captures.borrow_mut() = Some(tx);
        std::thread::spawn(move || {
            while let Ok((key, id, width, height)) = rx.recv_blocking() {
                let frame = desktop
                    .capture(&id)
                    .map(|frame| crate::thumbnail::resample(frame, width, height));
                if results.send_blocking((key, id, frame)).is_err() {
                    break;
                }
            }
        });
        let weak = Rc::downgrade(self);
        gtk::glib::spawn_future_local(async move {
            while let Ok((key, id, result)) = incoming.recv().await {
                let Some(c) = weak.upgrade() else {
                    break;
                };
                c.capture_pending.set(false);
                if !c.workspace_active.get() || !c.body.is_mapped() || *c.key.borrow() != key {
                    continue;
                }
                if result.is_err() {
                    c.capture_retry
                        .set(std::time::Instant::now() + std::time::Duration::from_secs(2));
                }
                if let Ok(frame) = result {
                    let bytes = gtk::glib::Bytes::from_owned(frame.rgba);
                    let texture = gdk::MemoryTexture::new(
                        frame.width as i32,
                        frame.height as i32,
                        gdk::MemoryFormat::R8g8b8a8,
                        &bytes,
                        frame.stride,
                    );
                    c.frames.borrow_mut().insert(id.0.clone(), texture.clone());
                    for (target, picture) in c.thumbnails.borrow().iter() {
                        if *target == id {
                            picture
                                .set_paintable(Some(&crate::thumbnail::Thumbnail::new(&texture)));
                        }
                    }
                }
            }
        });
    }
    fn highlight(&self) {
        let (at, pos, rate) = *self.clock.borrow();
        let position = pos + at.elapsed().as_secs_f64() * rate;
        let rows = self.lyric_rows.borrow();
        let current = rows.iter().rposition(|(time, _)| *time <= position);
        if self.active_line.replace(current) != current {
            for (i, (_, label)) in rows.iter().enumerate() {
                if Some(i) == current {
                    label.add_css_class("lyric-current");
                } else {
                    label.remove_css_class("lyric-current");
                }
            }
        }
        if std::time::Instant::now() >= *self.follow_after.borrow() {
            if let (Some(i), Some(scroll)) = (current, self.lyric_scroll.borrow().as_ref()) {
                let a = scroll.vadjustment();
                let allocation = rows[i].1.allocation();
                let target = (allocation.y() as f64 + allocation.height() as f64 / 2.
                    - a.page_size() / 2.)
                    .clamp(a.lower(), (a.upper() - a.page_size()).max(a.lower()));
                a.set_value(a.value() + (target - a.value()) * 0.45);
            }
        }
    }
    pub fn update(
        self: &Rc<Self>,
        row: Value,
        context: Value,
        style: Value,
        snapshot: Option<Snapshot>,
    ) {
        let key = if self.provider == "playerctl" {
            json!([row["id"], row["native"]["props"]["Metadata"]]).to_string()
        } else {
            json!([context["barKey"], context["module"], row["id"]]).to_string()
        };
        let changed = *self.key.borrow() != key;
        if changed {
            *self.key.borrow_mut() = key;
            *self.chart_trace.borrow_mut() = crate::crypto_chart::Trace::default();
            *self.chart.borrow_mut() = Value::Null;
            self.art.borrow_mut().clear();
            *self.lyrics.borrow_mut() = Value::Null;
            self.tokens.borrow_mut().clear();
            self.frames.borrow_mut().clear();
            *self.clock.borrow_mut() = (std::time::Instant::now(), 0., 0.);
        }
        let redraw = changed
            || if self.provider == "playerctl" {
                let previous = self.row.borrow();
                [
                    "PlaybackStatus",
                    "CanControl",
                    "CanPlay",
                    "CanPause",
                    "CanGoNext",
                    "CanGoPrevious",
                    "Shuffle",
                ]
                .iter()
                .any(|k| previous["native"]["props"][k] != row["native"]["props"][k])
            } else {
                *self.row.borrow() != row
            }
            || *self.style.borrow() != style
            || (self.provider == "workspaces" && *self.snapshot.borrow() != snapshot);
        *self.row.borrow_mut() = row;
        *self.context.borrow_mut() = context;
        *self.style.borrow_mut() = style;
        *self.snapshot.borrow_mut() = snapshot;
        if redraw {
            self.render();
        }
        if changed && self.body.is_mapped() {
            self.load();
        }
    }
    fn request(self: &Rc<Self>, kind: &str, mut request: Value) {
        if self.tokens.borrow().contains_key(kind) {
            return;
        }
        let n = self.sequence.get() + 1;
        self.sequence.set(n);
        let token = format!("gtk-{}-{kind}-{n}", self.provider);
        request["op"] = json!(kind);
        request["token"] = json!(token);
        self.tokens.borrow_mut().insert(kind.into(), token.clone());
        let weak = Rc::downgrade(self);
        let pending_kind = kind.to_owned();
        gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(30), move || {
            if let Some(c) = weak.upgrade() {
                if c.tokens.borrow().get(&pending_kind) == Some(&token) {
                    c.tokens.borrow_mut().remove(&pending_kind);
                    if pending_kind == "crypto" {
                        *c.chart.borrow_mut() = json!({"error":"timeout"});
                    }
                    if pending_kind == "lyrics" {
                        *c.lyrics.borrow_mut() = json!({"status":"error"});
                    }
                    if c.body.is_mapped() {
                        c.render();
                    }
                }
            }
        });
        if self.send.try_send(Event::LeftRequest(request)).is_err() {
            self.tokens.borrow_mut().remove(kind);
            if kind == "crypto" {
                *self.chart.borrow_mut() = json!({"error":"queue unavailable"});
            }
            if kind == "lyrics" {
                *self.lyrics.borrow_mut() = json!({"status":"error"});
            }
            self.render();
        }
    }
    fn load(self: &Rc<Self>) {
        if self.provider == "crypto"
            && self.chart.borrow().is_null()
            && self.row.borrow()["state"]["unavailable"] != true
        {
            self.request("crypto", json!({"context":*self.context.borrow()}));
        }
        if self.provider == "playerctl" {
            let row = self.row.borrow();
            let meta = &row["native"]["props"]["Metadata"];
            let url = s(&meta["mpris:artUrl"]);
            if !url.is_empty() && self.art.borrow().is_empty() {
                if url.starts_with("file:") {
                    *self.art.borrow_mut() = url.into();
                    self.render();
                } else {
                    self.request("artwork", json!({"url":url}));
                }
            }
            if self.show_lyrics.get() && self.lyrics.borrow().is_null() {
                self.request("lyrics",json!({"track":{"title":meta["xesam:title"],"artist":arr(&meta["xesam:artist"]).iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),"album":meta["xesam:album"],"duration":num(&meta["mpris:length"])/1_000_000.,"spotifyId":spotify_id(meta)}}));
            }
        }
    }
    pub fn receive(self: &Rc<Self>, v: &Value) {
        let kind = s(&v["kind"]);
        if !self.body.is_mapped()
            || self.tokens.borrow().get(kind).map(String::as_str) != v["token"].as_str()
        {
            return;
        }
        self.tokens.borrow_mut().remove(kind);
        if kind == "mediaPosition" {
            let clock = &v["clock"];
            if let Some(pos) = clock["position"].as_f64() {
                *self.clock.borrow_mut() = (
                    std::time::Instant::now(),
                    pos / 1_000_000.,
                    if clock["playing"] == true {
                        clock["rate"].as_f64().unwrap_or(1.)
                    } else {
                        0.
                    },
                );
                self.highlight();
            }
            return;
        }
        match kind {
            "crypto" => *self.chart.borrow_mut() = v["chart"].clone(),
            "artwork" => *self.art.borrow_mut() = s(&v["source"]).into(),
            "lyrics" => *self.lyrics.borrow_mut() = v["lyrics"].clone(),
            _ => return,
        }
        self.render();
    }
    pub fn content(&self) -> Content {
        let row = self.row.borrow();
        Content {
            title: match self.provider {
                "crypto" => format!("{}  ·  {}", s(&row["id"]), s(&row["text"]["value"])),
                "playerctl" => "Music".into(),
                _ => format!("Workspace {}", s(&row["id"])),
            },
            body: self.body.clone().upcast(),
            footer: None,
            preferred_width: if self.provider == "workspaces" {
                self.workspace_size().0
            } else if self.provider == "playerctl" {
                420.
            } else {
                440.
            },
            needs_keyboard: false,
        }
    }
    fn render(self: &Rc<Self>) {
        self.thumbnails.borrow_mut().clear();
        self.lyric_rows.borrow_mut().clear();
        self.lyric_scroll.borrow_mut().take();
        self.active_line.set(None);
        while let Some(w) = self.body.first_child() {
            self.body.remove(&w);
        }
        match self.provider {
            "crypto" => self.crypto(),
            "playerctl" => self.player(),
            _ => self.workspaces(),
        };
        if self.body.is_mapped() {
            let _ = self.send.try_send(Event::PanelCommand("resize".into()));
        }
    }
    fn crypto(self: &Rc<Self>) {
        let row = self.row.borrow();
        let changes = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        for (key, title) in [("1h", "1 hour"), ("24h", "24 hours"), ("7d", "1 week")] {
            let col = gtk::Box::new(gtk::Orientation::Vertical, 4);
            col.append(&label(title, "policy-muted"));
            col.append(&label(
                &row["changes"][key]
                    .as_f64()
                    .filter(|n| n.is_finite())
                    .map(|n| format!("{n:+.2}%"))
                    .unwrap_or("—".into()),
                "left-value",
            ));
            changes.append(&col);
        }
        self.body.append(&changes);
        let chart = self.chart.borrow().clone();
        let pts: Vec<(f64, f64)> = arr(&chart["points"])
            .iter()
            .filter_map(|p| Some((p[0].as_f64()?, p[1].as_f64()?)))
            .filter(|(x, y)| x.is_finite() && y.is_finite())
            .take(4096)
            .collect();
        let accent = gdk::RGBA::parse(s(&self.style.borrow()["accent"]))
            .unwrap_or(gdk::RGBA::new(1., 0.32, 0.65, 1.));
        self.body.append(&crate::crypto_chart::widget(
            self.chart_trace.clone(),
            pts.clone(),
            accent,
        ));
        if pts.len() < 2 && (!chart.is_null() || row["state"]["unavailable"] == true) {
            let msg = label(
                if row["state"]["unavailable"] == true {
                    "Prices unavailable"
                } else if chart.is_null() {
                    ""
                } else {
                    "History unavailable"
                },
                "policy-muted",
            );
            msg.set_wrap(false);

            msg.set_halign(gtk::Align::Center);
            self.body.append(&msg);
            let retry = button("Retry", "crypto-retry");
            let weak = Rc::downgrade(self);
            retry.connect_clicked(move |_| {
                if let Some(c) = weak.upgrade() {
                    if c.row.borrow()["state"]["unavailable"] == true {
                        let _ = c.send.try_send(Event::LeftRequest(
                            json!({"op":"cryptoRetry","context":*c.context.borrow()}),
                        ));
                    } else {
                        *c.chart.borrow_mut() = Value::Null;
                        c.load();
                        c.render();
                    }
                }
            });
            if !chart.is_null() || row["state"]["unavailable"] == true {
                self.body.append(&retry);
            }
        }
        let axis = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let a = label("24 hours ago", "policy-muted");
        a.set_hexpand(true);
        axis.append(&a);
        axis.append(&label("Latest sample", "policy-muted"));
        self.body.append(&axis);
    }
    fn player(self: &Rc<Self>) {
        let row = self.row.borrow();
        let props = &row["native"]["props"];
        self.body.set_spacing(0);
        // Square artwork inside the card border; controls retain their own strip.
        let art_height = (self.content().preferred_width - 2.) as i32;
        let display = gtk::Box::new(gtk::Orientation::Vertical, 0);
        display.set_size_request(-1, art_height);
        if self.show_lyrics.get() {
            let lyrics = self.lyrics.borrow();
            let text = if let Some(t) = lyrics["text"].as_str().filter(|s| !s.is_empty()) {
                t.to_owned()
            } else if !arr(&lyrics["lines"]).is_empty() {
                arr(&lyrics["lines"])
                    .iter()
                    .map(|l| s(&l["text"]))
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                match s(&lyrics["status"]) {
                    "instrumental" => "Instrumental",
                    "error" => "Lyrics unavailable",
                    "ready" | "missing" | "not-found" => "No lyrics found",
                    _ => "",
                }
                .into()
            };
            if text.is_empty() {
                let placeholder = crate::panel_view::control_symbol("lyrics");
                placeholder.set_widget_name("lyrics-placeholder");
                placeholder.add_css_class("lyrics-placeholder");
                placeholder.set_vexpand(true);
                placeholder.set_halign(gtk::Align::Center);
                placeholder.set_valign(gtk::Align::Center);
                display.append(&placeholder);
            } else {
                let scroll = gtk::ScrolledWindow::new();
                scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
                scroll.set_min_content_height(art_height);
                scroll.set_max_content_height(art_height);
                let words = label(&text, "left-lyrics");
                words.set_selectable(true);
                words.add_css_class("panel-interactive");
                let lines = arr(&lyrics["lines"]);
                if lines.iter().any(|l| l["time"].is_number()) {
                    let column = gtk::Box::new(gtk::Orientation::Vertical, 14);
                    column.set_margin_top(24);
                    column.set_margin_bottom(24);
                    column.set_margin_start(20);
                    column.set_margin_end(20);
                    for line in lines {
                        if let Some(time) = line["time"].as_f64() {
                            let l = label(s(&line["text"]), "left-lyrics");
                            l.add_css_class("lyric-line");
                            column.append(&l);
                            self.lyric_rows.borrow_mut().push((time, l));
                        }
                    }
                    scroll.set_child(Some(&column));
                } else {
                    scroll.set_child(Some(&words));
                }
                let motion =
                    gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
                let weak = Rc::downgrade(self);
                motion.connect_scroll(move |_, _, _| {
                    if let Some(c) = weak.upgrade() {
                        *c.follow_after.borrow_mut() =
                            std::time::Instant::now() + std::time::Duration::from_secs(4);
                    }
                    gtk::glib::Propagation::Proceed
                });
                scroll.add_controller(motion);
                *self.lyric_scroll.borrow_mut() = Some(scroll.clone());
                display.append(&scroll);
            }
        } else {
            let source = self.art.borrow();
            let path = if source.starts_with("file:") {
                url::Url::parse(&source)
                    .ok()
                    .and_then(|u| u.to_file_path().ok())
            } else if source.starts_with('/') {
                Some(std::path::PathBuf::from(&*source))
            } else {
                None
            };
            if let Some(path) = path.filter(|p| p.is_file()) {
                let picture = gtk::Picture::for_filename(path);
                picture.set_can_shrink(true);
                picture.set_content_fit(gtk::ContentFit::Cover);
                picture.set_size_request(-1, art_height);
                let frame = gtk::Overlay::new();
                let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
                spacer.set_height_request(art_height);
                frame.set_child(Some(&spacer));
                frame.add_overlay(&picture);
                frame.set_measure_overlay(&picture, false);
                frame.set_overflow(gtk::Overflow::Hidden);
                display.append(&frame);
            } else {
                let empty = label("♫", "left-art-empty");
                empty.set_halign(gtk::Align::Center);
                empty.set_valign(gtk::Align::Center);
                empty.set_vexpand(true);
                display.append(&empty);
            }
        }
        let middle = gtk::GestureClick::new();
        middle.set_button(2);
        let mut context = self.context.borrow().clone();
        context["nativeOwner"] = row["native"]["owner"].clone();
        context["action"] = json!("player.toggle");
        let send = self.send.clone();
        let can_toggle = props["CanControl"] == true
            && props[if props["PlaybackStatus"] == "Playing" {
                "CanPause"
            } else {
                "CanPlay"
            }] == true;
        middle.connect_released(move |_, _, _, _| {
            if can_toggle {
                let _ = send.try_send(Event::LeftRequest(
                    json!({"op":"mediaAction","context":context}),
                ));
            }
        });
        display.add_controller(middle);
        self.body.append(&display);
        let strip = gtk::CenterBox::new();
        strip.add_css_class("music-controls");
        strip.set_height_request(64);
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let lyrics = music_button("lyrics-toggle", false);
        lyrics.set_child(Some(&crate::panel_view::control_symbol("lyrics")));
        lyrics.update_property(&[gtk::accessible::Property::Label("Show lyrics")]);
        lyrics
            .child()
            .unwrap()
            .set_opacity(if self.show_lyrics.get() { 1. } else { 0.4 });
        strip.set_start_widget(Some(&lyrics));
        let weak = Rc::downgrade(self);
        lyrics.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.show_lyrics.set(!c.show_lyrics.get());
                c.start_ticker();
                c.load();
                c.render();
            }
        });

        for (name, symbol, enabled) in [
            (
                "previous",
                "skip_previous",
                props["CanControl"] == true && props["CanGoPrevious"] == true,
            ),
            (
                "toggle",
                if props["PlaybackStatus"] == "Playing" {
                    "pause"
                } else {
                    "play_arrow"
                },
                props["CanControl"] == true
                    && props[if props["PlaybackStatus"] == "Playing" {
                        "CanPause"
                    } else {
                        "CanPlay"
                    }] == true,
            ),
            (
                "next",
                "skip_next",
                props["CanControl"] == true && props["CanGoNext"] == true,
            ),
        ] {
            let b = music_button(name, true);
            #[cfg(test)]
            if name == "previous" {
                b.set_state_flags(
                    gtk::StateFlags::PRELIGHT | gtk::StateFlags::FOCUS_VISIBLE,
                    false,
                );
            }
            b.set_child(Some(&crate::panel_view::control_symbol(symbol)));
            b.set_sensitive(enabled);
            b.update_property(&[gtk::accessible::Property::Label(name)]);
            let mut ctx = self.context.borrow().clone();
            ctx["nativeOwner"] = row["native"]["owner"].clone();
            ctx["action"] = json!(format!("player.{name}"));
            let send = self.send.clone();
            b.connect_clicked(move |_| {
                let _ = send.try_send(Event::LeftRequest(
                    json!({"op":"mediaAction","context":ctx}),
                ));
            });
            controls.append(&b);
        }
        strip.set_center_widget(Some(&controls));
        let shuffle = music_button("shuffle", false);
        shuffle.set_child(Some(&crate::panel_view::control_symbol("shuffle")));
        shuffle.update_property(&[gtk::accessible::Property::Label("Shuffle")]);
        shuffle.set_visible(props["Shuffle"].is_boolean());
        shuffle.set_sensitive(props["CanControl"] == true);
        shuffle
            .child()
            .unwrap()
            .set_opacity(if props["Shuffle"] == true { 1. } else { 0.4 });
        let mut ctx = self.context.borrow().clone();
        ctx["nativeOwner"] = row["native"]["owner"].clone();
        ctx["action"] = json!("player.shuffle");
        let send = self.send.clone();
        shuffle.connect_clicked(move |_| {
            let _ = send.try_send(Event::LeftRequest(
                json!({"op":"mediaAction","context":ctx}),
            ));
        });
        strip.set_end_widget(Some(&shuffle));
        self.body.append(&strip);
    }
    fn workspace_size(&self) -> (f64, f64) {
        let snapshot = self.snapshot.borrow();
        let context = self.context.borrow();
        let output = snapshot.as_ref().and_then(|s| {
            s.outputs
                .iter()
                .find(|o| Some(o.name.as_str()) == context["monitor"].as_str())
        });
        let (w, h) = output
            .map(|o| (o.logical.width, o.logical.height))
            .unwrap_or((1920., 1080.));
        let height = self.style.borrow()["previewHeight"]
            .as_f64()
            .unwrap_or(360.)
            .min((h - 24.).max(120.));
        (
            (20. + (height - 20.) * w / h.max(1.))
                .min((w - 24.).max(200.))
                .clamp(200., 900.)
                .round(),
            height,
        )
    }
    fn workspaces(&self) {
        self.body.set_spacing(0);
        let snapshot = self.snapshot.borrow();
        let Some(snapshot) = snapshot.as_ref() else {
            self.body
                .append(&label("Window state unavailable", "policy-muted"));
            return;
        };
        let row = self.row.borrow();
        let context = self.context.borrow();
        let name = s(&row["workspace"]);
        let name = if name.is_empty() { s(&row["id"]) } else { name };
        let Some(output) = snapshot
            .outputs
            .iter()
            .find(|o| Some(o.name.as_str()) == context["monitor"].as_str())
        else {
            return;
        };
        let windows: Vec<_> = snapshot
            .windows
            .iter()
            .filter(|w| {
                w.mapped != Some(false)
                    && w.output.as_deref() == Some(output.id.as_str())
                    && w.workspace.as_ref().is_some_and(|ws| {
                        ws.name.as_deref() == Some(name)
                            || ws.id == name
                            || row["target"]["id"].as_str() == Some(&ws.id)
                    })
            })
            .collect();
        self.frames
            .borrow_mut()
            .retain(|id, _| windows.iter().any(|w| w.id.0 == *id));
        let (outer_width, outer_height) = self.workspace_size();
        let (width, height) = (outer_width - 2., outer_height - 2.);
        let scale = ((width - 18.) / output.logical.width.max(1.))
            .min((height - 18.) / output.logical.height.max(1.));
        let desktop_width = output.logical.width * scale;
        let desktop_height = output.logical.height * scale;
        let scene = gtk::Fixed::new();
        scene.set_size_request(width as i32, height as i32);
        scene.add_css_class("workspace-scene");
        scene.set_overflow(gtk::Overflow::Hidden);
        // Windows may extend beyond the output; their natural size must not
        // enlarge the preview viewport or change the panel's aspect ratio.
        let desktop = gtk::Fixed::new();
        let viewport = gtk::Overlay::new();
        let bounds = gtk::Box::new(gtk::Orientation::Vertical, 0);
        bounds.set_size_request(desktop_width as i32, desktop_height as i32);
        viewport.set_child(Some(&bounds));
        viewport.add_overlay(&desktop);
        viewport.set_measure_overlay(&desktop, false);
        viewport.add_css_class("workspace-desktop");
        viewport.set_overflow(gtk::Overflow::Hidden);
        scene.put(
            &viewport,
            ((width - desktop_width) / 2.).round(),
            ((height - desktop_height) / 2.).round(),
        );
        let overflow_left = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let overflow_right = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        for strip in [&overflow_left, &overflow_right] {
            strip.add_css_class("workspace-overflow");
        }
        let max = context["maxWindows"].as_u64().unwrap_or(12).clamp(1, 48) as usize;
        let mut visible = 0;
        for w in windows {
            let r = w.geometry;
            let on_screen = w.hidden != Some(true)
                && visible < max
                && r.is_some_and(|r| {
                    r.x < output.logical.x + output.logical.width - 8.
                        && r.y < output.logical.y + output.logical.height
                        && r.x + r.width > output.logical.x + 8.
                        && r.y + r.height > output.logical.y
                });
            let b = button(
                "",
                if on_screen {
                    "workspace-tile"
                } else {
                    "workspace-window"
                },
            );
            b.add_css_class("workspace-tile");
            b.update_property(&[gtk::accessible::Property::Label(
                w.title.as_deref().unwrap_or("Window"),
            )]);
            let send = self.actions.clone();
            let id = w.id.clone();
            let ui = self.send.clone();
            b.connect_clicked(move |_| {
                if send.try_send(Action::Window(id.clone())).is_ok() {
                    let _ = ui.try_send(Event::PanelCommand("close".into()));
                }
            });
            let icon = application_icon(
                w.app_id
                    .as_deref()
                    .or(w.initial_app_id.as_deref())
                    .unwrap_or(""),
            );
            icon.set_pixel_size(24);
            if on_screen {
                visible += 1;
                let r = r.unwrap();
                let overlay = gtk::Overlay::new();
                overlay.set_child(Some(&gtk::Box::new(gtk::Orientation::Vertical, 0)));
                let picture = gtk::Picture::new();
                picture.set_can_shrink(true);
                picture.set_content_fit(gtk::ContentFit::Fill);
                if let Some(texture) = self.frames.borrow().get(&w.id.0) {
                    picture.set_paintable(Some(&crate::thumbnail::Thumbnail::new(texture)));
                }
                overlay.add_overlay(&picture);
                overlay.set_measure_overlay(&picture, false);
                b.set_child(Some(&overlay));
                b.set_size_request(
                    (r.width * scale).max(24.) as i32,
                    (r.height * scale).max(24.) as i32,
                );
                desktop.put(
                    &b,
                    ((r.x - output.logical.x) * scale).round(),
                    ((r.y - output.logical.y) * scale).round(),
                );
                self.thumbnails.borrow_mut().push((w.id.clone(), picture));
            } else {
                icon.set_pixel_size(20);
                icon.set_halign(gtk::Align::Center);
                icon.set_valign(gtk::Align::Center);
                b.set_valign(gtk::Align::Center);
                b.remove_css_class("workspace-tile");
                b.set_focus_on_click(false);
                overflow_tooltip(
                    &b,
                    w.title
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(w.app_id.as_deref().unwrap_or("Window")),
                );
                #[cfg(test)]
                b.set_state_flags(gtk::StateFlags::PRELIGHT, false);
                b.set_child(Some(&icon));
                let close = gtk::GestureClick::new();
                close.set_button(2);
                let send = self.actions.clone();
                let id = w.id.clone();
                close.connect_released(move |_, _, _, _| {
                    let _ = send.try_send(Action::Close(id.clone()));
                });
                b.add_controller(close);
                let left = r.is_none_or(|r| {
                    r.x + r.width / 2. < output.logical.x + output.logical.width / 2.
                });
                if left {
                    overflow_left.append(&b);
                } else {
                    overflow_right.append(&b);
                }
            }
        }
        for (left, strip) in [(true, overflow_left), (false, overflow_right)] {
            if strip.first_child().is_none() {
                continue;
            }
            // This strip is not parented under .panel-card yet, so GTK's
            // measured button padding would still come from the default theme.
            let mut count = 0;
            let mut child = strip.first_child();
            while let Some(widget) = child {
                count += 1;
                child = widget.next_sibling();
            }
            let natural = (count * 24 + (count - 1) * 4) as f64;
            strip.set_halign(gtk::Align::Center);
            let notch_width = (natural + 24.).min(desktop_width / 2. - 20.);
            let notch =
                crate::panel_view::workspace_notch(&self.style.borrow(), notch_width as i32);
            let scroll = gtk::ScrolledWindow::new();
            scroll.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
            scroll.set_min_content_height(24);
            scroll.set_child(Some(&strip));
            scroll.set_margin_start(20);
            scroll.set_margin_end(20);
            scroll.set_valign(gtk::Align::Center);
            notch.add_overlay(&scroll);
            desktop.put(
                &notch,
                if left {
                    desktop_width / 2. - 10. - notch_width - 8.
                } else {
                    desktop_width / 2. + 10. - 8.
                },
                desktop_height - 36.,
            );
        }
        self.body.append(&scene);
    }
}

#[cfg(test)]
pub fn fixture(
    provider: &'static str,
    send: async_channel::Sender<Event>,
    actions: async_channel::Sender<Action>,
) -> Rc<Card> {
    let c = Card::new(provider, send, actions);
    let row = match provider {
        "crypto" => {
            json!({"id":"BTC/USD","text":{"value":"67,432"},"changes":{"1h":0.42,"24h":-1.17,"7d":5.31}})
        }
        "playerctl" => {
            json!({"id":"org.mpris.MediaPlayer2.fixture","native":{"props":{"PlaybackStatus":"Playing","CanControl":true,"CanPause":true,"CanPlay":true,"CanGoPrevious":true,"Shuffle":false,"CanGoNext":true,"Metadata":{"xesam:title":"Evening light","xesam:artist":["The Harbour"],"xesam:album":"Night crossings","mpris:length":240000000}}}})
        }
        _ => json!({"id":"7"}),
    };
    let mut snapshot: Snapshot=serde_json::from_value(json!({"generation":"fixture","revision":1,"workspaces":[],"outputs":[{"id":"opaque-output-17","name":"synthetic","description":null,"logical":{"x":0,"y":0,"width":1920,"height":1080},"width":1920,"height":1080,"scale":1,"transform":0,"focused":true,"workspace":{"id":"fake-space","name":"7"}}],"windows":[{"id":"fixture/window/editor","title":"Migration notes","app_id":"Editor","output":"opaque-output-17","workspace":{"id":"fake-space","name":"7"},"geometry":{"x":0,"y":0,"width":1200,"height":1080},"mapped":true,"hidden":false},{"id":"fixture/window/terminal","title":"Build output","app_id":"Terminal","output":"opaque-output-17","workspace":{"id":"fake-space","name":"7"},"geometry":{"x":1200,"y":0,"width":720,"height":1080},"mapped":true,"hidden":false}]})).unwrap();
    let mut offscreen = snapshot.windows[0].clone();
    offscreen.id = tjik_desktop::WindowId("fixture/window/overflow".into());
    // Hyprland scrolling leaves a two-pixel sliver, still overflow in the original.
    offscreen.geometry.as_mut().unwrap().x = -1198.;
    snapshot.windows.push(offscreen);
    c.update(row,json!({"barKey":"fixture","module":provider,"item":if provider=="crypto"{"BTC/USD"}else{"7"},"monitor":"synthetic"}),json!({"accent":"#ff53a6"}),Some(snapshot));
    if provider == "workspaces" {
        for (index, id) in ["fixture/window/editor", "fixture/window/terminal"]
            .iter()
            .enumerate()
        {
            let mut rgba = vec![0u8; 320 * 180 * 4];
            for y in 0..180 {
                for x in 0..320 {
                    let line = y > 30 && y % 16 < 3 && x > 16 && x < 240 - ((y / 16) % 4) * 24;
                    let rgb = if y < 20 {
                        [62, 56, 84]
                    } else if line {
                        if index == 0 {
                            [155, 145, 185]
                        } else {
                            [110, 180, 160]
                        }
                    } else {
                        [31, 29, 46]
                    };
                    let offset = (y * 320 + x) * 4;
                    rgba[offset..offset + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
            }
            let texture = gdk::MemoryTexture::new(
                320,
                180,
                gdk::MemoryFormat::R8g8b8a8,
                &gtk::glib::Bytes::from_owned(rgba),
                320 * 4,
            );
            c.frames.borrow_mut().insert((*id).into(), texture);
        }
    }
    if provider == "crypto" && std::env::var_os("BARTJIK_EMPTY_USAGE").is_none() {
        *c.chart.borrow_mut() = json!({"points":(0..144).map(|i|json!([i*600000,67432.+(i as f64/12.).sin()*800.+i as f64*8.])).collect::<Vec<_>>(),"updated":chrono::Local::now().timestamp()});
    }
    if provider == "playerctl" {
        *c.art.borrow_mut() = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/player-cover.svg")
            .to_string_lossy()
            .into_owned();
        *c.lyrics.borrow_mut() = json!({"status":"ready","lines":[{"time":0,"text":"Evening light across the water"},{"time":5,"text":"Quiet footsteps on the quay"}],"text":"Evening light across the water\nQuiet footsteps on the quay\n\nWindows glow along the harbour\nHome is where the lights will be"});
    }
    if provider == "playerctl" && std::env::var_os("BARTJIK_EMPTY_USAGE").is_some() {
        c.show_lyrics.set(true);
        *c.lyrics.borrow_mut() = Value::Null;
    }
    c.render();
    c
}
#[cfg(test)]
pub fn exercise(
    c: &Rc<Card>,
    events: &async_channel::Receiver<Event>,
    actions: &async_channel::Receiver<Action>,
) {
    fn find(w: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
        if w.widget_name() == name {
            return Some(w.clone());
        }
        let mut child = w.first_child();
        while let Some(w) = child {
            if let Some(v) = find(&w, name) {
                return Some(v);
            }
            child = w.next_sibling();
        }
        None
    }
    let click = |name: &str| {
        find(c.body.upcast_ref(), name)
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap()
            .emit_clicked()
    };
    match c.provider {
        "playerctl" => {
            assert_eq!(find(c.body.upcast_ref(), "previous").unwrap().height(), 40);
            assert_eq!(find(c.body.upcast_ref(), "previous").unwrap().width(), 42);
            assert_eq!(
                find(c.body.upcast_ref(), "lyrics-toggle").unwrap().height(),
                32
            );
            if c.show_lyrics.get() {
                assert!(find(c.body.upcast_ref(), "lyrics-placeholder").is_some());
                click("lyrics-toggle");
                *c.lyrics.borrow_mut() = json!({"status":"ready","lines":[{"time":0,"text":"Evening light across the water"},{"time":5,"text":"Quiet footsteps on the quay"}]});
            }
            click("next");
            let mut found = false;
            while let Ok(e) = events.try_recv() {
                if let Event::LeftRequest(v) = e {
                    if v["op"] == "mediaAction" {
                        assert_eq!(v["context"]["action"], "player.next");
                        found = true;
                    }
                }
            }
            assert!(found);
            click("lyrics-toggle");
            assert!(c.show_lyrics.get());
            *c.clock.borrow_mut() = (std::time::Instant::now(), 6., 0.);
            c.highlight();
            assert_eq!(c.active_line.get(), Some(1));
            assert!(c.lyric_rows.borrow()[1].1.has_css_class("lyric-current"));
            let until = std::time::Instant::now() + std::time::Duration::from_millis(150);
            while std::time::Instant::now() < until {
                while gtk::glib::MainContext::default().pending() {
                    gtk::glib::MainContext::default().iteration(false);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let window = c.body.root().unwrap().downcast::<gtk::Window>().unwrap();
            let snapshot = gtk::Snapshot::new();
            window.snapshot_child(&window.child().unwrap(), &snapshot);
            window
                .renderer()
                .unwrap()
                .render_texture(&snapshot.to_node().unwrap(), None)
                .save_to_png("/tmp/bartjik-playerctl-lyrics-primitives.png")
                .unwrap();
            click("lyrics-toggle");
            assert!(!c.show_lyrics.get());
            click("shuffle");
            assert!(std::iter::from_fn(|| events.try_recv().ok()).any(
                |e| matches!(e, Event::LeftRequest(v) if v["context"]["action"] == "player.shuffle")
            ));
        }
        "workspaces" => {
            let notch = find(c.body.upcast_ref(), "workspace-notch").unwrap();
            let button = find(c.body.upcast_ref(), "workspace-window")
                .unwrap()
                .downcast::<gtk::Button>()
                .unwrap();
            assert_eq!(button.tooltip_text().as_deref(), Some("Migration notes"));
            let image = button.child().unwrap();
            let n = notch.compute_bounds(&c.body).unwrap();
            let i = image.compute_bounds(&c.body).unwrap();
            assert!(
                (n.x() + n.width() / 2. - i.x() - i.width() / 2.).abs() <= 0.5,
                "overflow icon horizontally centered"
            );
            assert!(
                (n.y() + n.height() / 2. - i.y() - i.height() / 2.).abs() <= 0.5,
                "overflow icon vertically centered"
            );
            let picture = &c.thumbnails.borrow()[0].1;
            let snapshot = gtk::Snapshot::new();
            picture.paintable().unwrap().snapshot(&snapshot, 64., 64.);
            let node = snapshot
                .to_node()
                .unwrap()
                .downcast::<gtk::gsk::TextureScaleNode>()
                .unwrap();
            assert_eq!(node.filter(), gtk::gsk::ScalingFilter::Linear);
            let tile = find(c.body.upcast_ref(), "workspace-tile").unwrap();
            fn contains_image(w: &gtk::Widget) -> bool {
                if w.is::<gtk::Image>() {
                    return true;
                }
                let mut child = w.first_child();
                while let Some(w) = child {
                    if contains_image(&w) {
                        return true;
                    }
                    child = w.next_sibling();
                }
                false
            }
            assert!(
                !contains_image(&tile),
                "workspace capture placeholders must not show application icons"
            );
            click("workspace-window");
            assert!(
                matches!(actions.try_recv().unwrap(),Action::Window(id) if id.0=="fixture/window/overflow")
            );
            let window = find(c.body.upcast_ref(), "workspace-window").unwrap();
            let controllers = window.observe_controllers();
            for i in 0..controllers.n_items() {
                if let Some(gesture) = controllers.item(i).and_downcast::<gtk::GestureClick>() {
                    if gesture.button() == 2 {
                        gesture.emit_by_name::<()>("released", &[&1i32, &0f64, &0f64]);
                    }
                }
            }
            assert!(
                matches!(actions.try_recv().unwrap(),Action::Close(id) if id.0=="fixture/window/overflow")
            );
        }
        _ => {
            let before = c.chart.borrow().clone();
            c.receive(&json!({"kind":"crypto","token":"stale-request","chart":{"points":[]}}));
            assert_eq!(*c.chart.borrow(), before);
        }
    }
    c.tokens
        .borrow_mut()
        .insert("crypto".into(), "pending".into());
    c.body.set_visible(false);
    assert!(c.tokens.borrow().is_empty());
}
