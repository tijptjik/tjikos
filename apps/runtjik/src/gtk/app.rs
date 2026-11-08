use crate::{
    control, desktop, views,
    worker::{Request, Worker},
    Event,
};
use anyhow::Result;
use gtk::{gdk, gio, glib, prelude::*};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::atomic::Ordering,
    time::Duration,
};
use tjik_gtk::{
    results::{self, Layout},
    surface, theme,
};

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
struct Ui {
    window: gtk::ApplicationWindow,
    pointer: gtk::EventControllerMotion,
    search: gtk::Entry,
    mark: tjik_gtk::mark::Mark,
    topbar: gtk::Box,
    model: gtk::StringList,
    selection: gtk::SingleSelection,
    list: gtk::ListView,
    grid: gtk::GridView,
    stack: gtk::Stack,
    results: gtk::Stack,
    rich: gtk::ScrolledWindow,
    word_header: gtk::Box,
    word_key: RefCell<String>,
    hidden_sources: Rc<RefCell<std::collections::HashSet<String>>>,
    message: gtk::Label,
    startup: gtk::Picture,
    effects: gtk::Overlay,
    composing: Cell<bool>,
    composition_commit: Cell<bool>,
    tasks: Rc<tasktjik_gtk::TaskView>,
    css: gtk::CssProvider,
    worker: Worker,
    rows: RefCell<Vec<Value>>,
    settings: RefCell<Value>,
    query: RefCell<String>,
    display_query: RefCell<String>,
    windows_for: RefCell<String>,
    generation: Cell<u64>,
    ready: Cell<bool>,
    busy: Cell<bool>,
    changing_text: Cell<bool>,
    debounce: RefCell<Option<glib::SourceId>>,
    refresh: RefCell<Option<glib::SourceId>>,
    error: RefCell<String>,
    demo: bool,
    last_activation: RefCell<String>,
    layer: bool,
    pinned: Cell<bool>,
    opened: Cell<bool>,
    restoring: Cell<bool>,
    desktop_pending: Cell<bool>,
    desktop_sequence: Cell<u64>,
    view: RefCell<String>,
    columns: Cell<u32>,
    factory_key: RefCell<String>,
    total: Cell<u64>,
    next_offset: Cell<u64>,
    has_more: Cell<bool>,
    append: Cell<bool>,
    indexed: Cell<u64>,
    history: RefCell<Vec<String>>,
    history_cursor: Cell<usize>,
    history_draft: RefCell<String>,
    auto: Cell<bool>,
    pending_action: RefCell<Option<String>>,
    app: gtk::Application,
}
impl Ui {
    fn recovery_state(&self) {
        if !self.restoring.get() {
            crate::recovery::remember(
                json!({"query":self.query.borrow().as_str(),"visible":self.opened.get(),"pinned":self.pinned.get(),"view":self.view.borrow().as_str()}),
            );
        }
    }
    fn status(&self) -> Value {
        let scroll = self
            .results
            .visible_child()
            .and_downcast::<gtk::ScrolledWindow>();
        let scroll_y = scroll
            .as_ref()
            .map(|s| s.vadjustment().value())
            .unwrap_or(0.);
        let row_pitch = scroll.as_ref().and_then(|s| {
            s.child()
                .and_then(|content| tjik_gtk::scroll::row_pitch(&content, s))
        });
        let selected_bounds = scroll.as_ref().and_then(|viewport| {
            let id = self.rows.borrow().get(self.selection.selected() as usize)?["id"].as_str()?.to_owned();
            let row = tjik_gtk::removal::rows(&self.results).into_iter()
                .find(|row| row.widget_name() == id && row.is_mapped())?;
            let bounds = row.compute_bounds(viewport)?;
            Some(json!({"top": bounds.y(), "bottom": bounds.y() + bounds.height(),
                "viewportHeight": viewport.height(), "highlightHidden": viewport.has_css_class("scrolling")}))
        });
        let result_geometry: Vec<Value> = if self.demo {
            tjik_gtk::removal::rows(&self.results).iter().filter(|w|w.is_mapped()).filter_map(|row| {
                let icon = row.first_child()?.compute_bounds(&self.window)?;
                let text = row.last_child()?.first_child()?.compute_bounds(&self.window)?;
                Some(json!({"id":row.widget_name().as_str(),"iconX":icon.x(),"iconWidth":icon.width(),"textX":text.x(),"texture":row.first_child().and_then(|slot|slot.first_child()).and_downcast::<gtk::Image>().is_some_and(|image|image.paintable().is_some())}))
            }).collect()
        } else {
            vec![]
        };
        json!({"lastActivation":self.last_activation.borrow().as_str(),"selectedBounds":selected_bounds,"desktop_sequence":self.desktop_sequence.get(),"pinAngle":self.mark.angle(),"captureUi":if self.demo {views::inspect(&self.word_header,&self.results,&self.window)}else{Value::Null},"resultGeometry":result_geometry,"wordUi": if self.demo { views::inspect(&self.word_header, &self.rich, &self.window) } else { Value::Null }, "scrollY":scroll_y,"rowPitch":row_pitch,"input":self.search.text().as_str(),"composing":self.composing.get(),"startup":self.startup.is_visible(),"renderer":"gtk4","surface":if self.layer{"layer"}else{"window"},"running":true,"pid":std::process::id(),"ready":self.ready.get(),"visible":self.window.is_visible(),"focused":self.window.is_active(),"hovered":self.pointer.contains_pointer(),"busy":self.busy.get(),"demo":self.demo,"query":self.query.borrow().as_str(),"resultQuery":self.display_query.borrow().as_str(),"total":self.total.get(),"indexed":self.indexed.get(),"view":self.view.borrow().as_str(),"columns":self.columns.get(),"pinned":self.pinned.get(),"app_state":self.tasks.state(),"rows":self.rows.borrow().clone(),"selected":self.selection.selected(),"backend_error":self.error.borrow().as_str(),"message":self.message.text().as_str()})
    }
    fn message(&self, text: &str) {
        self.message.set_text(text);
        self.message.set_visible(!text.is_empty());
    }
    fn next_generation(&self) {
        let n = self.generation.get() + 1;
        self.generation.set(n);
        self.worker.generation.store(n, Ordering::Relaxed);
    }
    fn send(&self, value: Value) {
        self.busy.set(value["op"] != "remember-command");
        if self
            .worker
            .send
            .try_send(Request {
                generation: self.generation.get(),
                value,
            })
            .is_err()
        {
            self.busy.set(false);
            self.message("Backend is busy or unavailable. Try again.");
        }
    }
    fn cancel_timer(&self) {
        if let Some(id) = self.debounce.borrow_mut().take() {
            id.remove();
        }
    }
    fn cancel_refresh(&self) {
        if let Some(id) = self.refresh.borrow_mut().take() {
            id.remove();
        }
    }
    fn set_text(&self, text: &str) {
        if self.composing.get() {
            return;
        }
        self.changing_text.set(true);
        self.search.set_text(text);
        self.search.set_position(-1);
        self.changing_text.set(false);
        *self.query.borrow_mut() = text.into();
    }
    fn query(self: &Rc<Self>, delay: bool) {
        if self.composing.get() {
            return;
        }
        self.cancel_timer();
        self.cancel_refresh();
        self.next_generation();
        self.busy.set(true);
        self.append.set(false);
        self.windows_for.borrow_mut().clear();
        *self.query.borrow_mut() = self.search.text().to_string();
        self.recovery_state();
        let weak = Rc::downgrade(self);
        let submit = move || {
            if let Some(ui) = weak.upgrade() {
                ui.debounce.borrow_mut().take();
                ui.send(json!({"op":"query","query":ui.query.borrow().as_str()}));
            }
        };
        if delay {
            let word = self.query.borrow().starts_with("/define ")
                || self.query.borrow().starts_with("/synonym ");
            *self.debounce.borrow_mut() = Some(glib::timeout_add_local_once(
                Duration::from_millis(if word { 350 } else { 60 }),
                submit,
            ));
        } else {
            submit();
        }
    }
    fn lookup(self: &Rc<Self>, query: &str) {
        self.set_text(query);
        self.auto.set(false);
        self.query(false);
        self.search.grab_focus();
    }
    fn remember(&self) {
        let query = self.query.borrow().clone();
        if self.settings.borrow()["history"]["enabled"] == false
            || query.trim().is_empty()
            || !query.starts_with(['!', '/', '?'])
        {
            return;
        }
        {
            let mut history = self.history.borrow_mut();
            history.retain(|q| q != &query);
            history.push(query.clone());
            if history.len() > 100 {
                history.remove(0);
            }
        }
        self.history_cursor.set(usize::MAX);
        if !self.demo {
            self.send(json!({"op":"remember-command","query":query}));
        }
    }
    fn browse_history(self: &Rc<Self>, delta: i32) {
        let history = self.history.borrow().clone();
        if history.is_empty() {
            return;
        }
        let mut at = self.history_cursor.get();
        if at == usize::MAX {
            *self.history_draft.borrow_mut() = self.search.text().into();
            at = history.len();
        }
        let at = (at as i32 + delta).clamp(0, history.len() as i32) as usize;
        self.history_cursor.set(at);
        let text = history
            .get(at)
            .cloned()
            .unwrap_or_else(|| self.history_draft.borrow().clone());
        self.lookup(&text);
    }
    fn show(self: &Rc<Self>) {
        if !self.opened.replace(true) {
            self.set_text("");
            self.next_generation();
            self.append.set(false);
            self.send(json!({"op":"resume"}));
        }
        self.pinned.set(false);
        self.mark.pinned(false);
        surface::keyboard(&self.window, true);
        desktop::place(
            &self.window,
            self.settings.borrow()["ui"]["monitor"]
                .as_str()
                .unwrap_or("focused"),
        );
        self.window.present();
        self.search.grab_focus();
        self.watch();
    }
    fn watch(self: &Rc<Self>) {
        self.recovery_state();
        if !self.layer {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(100), move || {
            if let Some(ui) = weak.upgrade() {
                if ui.window.is_visible() {
                    desktop::watch(ui.pinned.get(), ui.demo);
                }
            }
        });
    }
    fn hide(&self) {
        tjik_gtk::removal::cancel(&self.effects);
        self.cancel_timer();
        self.cancel_refresh();
        self.remember();
        self.opened.set(false);
        self.recovery_state();
        self.window.set_visible(false);
        if self.settings.borrow()["ui"]["keep_alive"] == false {
            self.app.quit();
        }
    }
    fn outside(&self) {
        if self.pinned.get() || self.settings.borrow()["ui"]["close_on_outside_click"] == false {
            surface::keyboard(&self.window, false);
        } else {
            self.hide();
        }
    }
    fn selected(&self) -> Option<Value> {
        self.rows
            .borrow()
            .get(self.selection.selected() as usize)
            .cloned()
    }
    fn gallery(&self) -> bool {
        let q = self.display_query.borrow();
        q.contains(" history")
            && [
                "/grab",
                "/shot",
                "/screenshot",
                "/ss",
                "/snap",
                "/rec",
                "/screenrecord",
            ]
            .iter()
            .any(|p| q.starts_with(p))
    }
    fn activate(self: &Rc<Self>, action: &str) {
        if !self.ready.get() {
            return;
        }
        if self.busy.get() || self.query.borrow().as_str() != self.display_query.borrow().as_str() {
            *self.pending_action.borrow_mut() = Some(action.into());
            self.query(false);
            return;
        }
        let Some(row) = self.selected() else { return };
        if self.demo {
            *self.last_activation.borrow_mut() = s(&row, "id").to_owned();
        }
        if row["disabled"] == true {
            return;
        }
        if row["empty_state"] == true {
            self.lookup(if s(&row, "provider") == "Recorder" {
                "/rec"
            } else {
                "/grab"
            });
            return;
        }
        self.cancel_timer();
        self.cancel_refresh();
        self.remember();
        self.next_generation();
        if s(&row, "id") == "Recorder:stop" && !self.demo {
            self.message("Stopping recording…");
            let core = std::env::var("RUNTJIK_HOME").unwrap_or_default() + "/runtjik-core";
            std::thread::spawn(move || {
                let _ = std::process::Command::new(core)
                    .arg("stop-recording")
                    .status();
            });
            self.schedule_refresh(200);
            return;
        }
        let app = ["apps", "commands"].contains(&s(&row, "provider"));
        let parent = self.windows_for.borrow().clone();
        let mode = if action == "close" {
            "close"
        } else if action == "copy" || action == "alternate" && self.gallery() {
            "copy"
        } else if app {
            if action == "alternate" || action == "launch" {
                "launch"
            } else {
                "switch"
            }
        } else if action == "alternate" {
            "switch"
        } else {
            "launch"
        };
        let prepare = app && parent.is_empty() && mode == "switch";
        let request = if parent.is_empty() {
            json!({"op":"activate","id":row["id"],"mode":mode,"prepare":prepare})
        } else {
            json!({"op":"activate","id":parent,"address":row["id"],"mode":if mode=="close"{"close"}else{"switch"}})
        };
        // Unmap before launching/capturing so focus and capture targets stay intact.
        let unmap = !self.demo
            && mode != "close"
            && mode != "copy"
            && !prepare
            && row["keep_open"] != true
            && (!s(&row, "id").starts_with('/') || !parent.is_empty());
        if unmap {
            if self.pinned.get() && row["hide_launcher"] != true {
                surface::keyboard(&self.window, false);
            } else {
                self.window.set_visible(false);
            }
            self.busy.set(true);
            let weak = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_millis(80), move || {
                if let Some(ui) = weak.upgrade() {
                    ui.send(request);
                }
            });
        } else {
            self.send(request);
        }
    }
    fn move_selection(self: &Rc<Self>, delta: i32) {
        self.results.add_css_class("keyboard-navigation");
        if let Some(viewport) = self.results.visible_child() {
            viewport.remove_css_class("scrolling");
        }
        let length = self.model.n_items() as i32;
        if length == 0 {
            return;
        }
        let current = self.selection.selected().min(length as u32 - 1) as i32;
        let mut next = current + delta;
        if self.word_header.is_visible() {
            let rows = self.rows.borrow();
            for _ in 0..length {
                let row = &rows[next.rem_euclid(length) as usize];
                if row["disabled"] != true
                    && !self.hidden_sources.borrow().contains(s(row, "word_source"))
                {
                    break;
                }
                next += delta.signum();
            }
        }
        if next >= length && self.has_more.get() {
            self.load_more();
            return;
        }
        let at = next.rem_euclid(length) as u32;
        self.selection.set_selected(at);
        if self.results.visible_child_name().as_deref() == Some("grid") {
            self.grid.scroll_to(at, gtk::ListScrollFlags::NONE, None);
        } else if self.results.visible_child_name().as_deref() == Some("list") {
            self.list.scroll_to(at, gtk::ListScrollFlags::NONE, None);
        } else if self.word_header.is_visible() {
            views::reveal_word(&self.rich, at);
        }
    }
    fn load_more(&self) {
        if !self.has_more.get() || self.busy.get() {
            return;
        }
        self.append.set(true);
        self.send(json!({"op":"query","query":self.query.borrow().as_str(),"offset":self.next_offset.get()}));
    }
    fn replace_rows(&self, rows: Vec<Value>, same: bool) {
        let old = self
            .rows
            .borrow()
            .get(self.selection.selected() as usize)
            .map(|r| r["id"].clone());
        let at = if same {
            old.and_then(|id| rows.iter().position(|r| r["id"] == id))
                .unwrap_or(0)
        } else {
            0
        };
        if same && *self.rows.borrow() == rows {
            return;
        }
        tjik_gtk::removal::cancel(&self.effects);
        if same {
            for widget in tjik_gtk::removal::rows(&self.results) {
                if !rows.iter().any(|row| s(row, "id") == widget.widget_name()) {
                    tjik_gtk::removal::animate(&self.effects, &widget, self.gallery());
                }
            }
        }
        let strings: Vec<_> = rows.iter().map(Value::to_string).collect();
        let refs: Vec<_> = strings.iter().map(String::as_str).collect();
        self.model.splice(0, self.model.n_items(), &refs);
        *self.rows.borrow_mut() = rows;
        self.selection.set_selected(at as u32);
    }
    fn configure(self: &Rc<Self>, settings: &Value, reset: bool) {
        if !settings.is_object() {
            return;
        }
        let monitor_changed = self.settings.borrow()["ui"]["monitor"] != settings["ui"]["monitor"];
        let changed = *self.settings.borrow() != *settings;
        *self.settings.borrow_mut() = settings.clone();
        if reset {
            *self.view.borrow_mut() = settings["ui"]["view"].as_str().unwrap_or("grid").into();
        }
        if !changed {
            return;
        }
        if self.layer && monitor_changed {
            desktop::place(
                &self.window,
                settings["ui"]["monitor"].as_str().unwrap_or("focused"),
            );
        }
        self.factory_key.borrow_mut().clear();
        let colors: theme::Theme =
            serde_json::from_value(settings["theme"].clone()).unwrap_or_default();
        let font = settings["ui"]["font"].as_str().unwrap_or("tijpset");
        let size = settings["ui"]["font_size"].as_i64().unwrap_or(14) as i32;
        theme::apply(&self.css, &colors, font, size);
        self.window.set_default_size(
            settings["ui"]["width"]
                .as_i64()
                .unwrap_or(720)
                .clamp(360, 1600) as i32,
            settings["ui"]["height"]
                .as_i64()
                .unwrap_or(600)
                .clamp(300, 1200) as i32,
        );
        let columns = settings["ui"]["columns"].as_u64().unwrap_or(5).clamp(1, 12) as u32;
        self.columns.set(columns);
        self.grid.set_min_columns(1);
        self.grid.set_max_columns(columns);
        self.grid.set_min_columns(columns);
        let weak = Rc::downgrade(self);
        let icon = settings["ui"]["icon_size"].as_i64().unwrap_or(52) as i32;
        self.grid
            .set_factory(Some(&results::factory(Layout::Grid, icon, move |i| {
                if let Some(ui) = weak.upgrade() {
                    ui.selection.set_selected(i);
                }
            })));
    }
    fn layout(self: &Rc<Self>, value: &Value) {
        self.word_header.set_visible(false);
        let app_mode = s(value, "app_mode") == "Todo";
        self.topbar.set_visible(!app_mode);
        self.stack
            .set_visible_child_name(if app_mode { "tasks" } else { "launcher" });
        if app_mode {
            self.tasks.update(&value["app_state"]);
            return;
        }
        let q = self.display_query.borrow().clone();
        if !value["statistics"].is_null() {
            let colors =
                serde_json::from_value(self.settings.borrow()["theme"].clone()).unwrap_or_default();
            self.rich.set_child(Some(&views::stats(
                &value["statistics"],
                &colors,
                self.indexed.get(),
            )));
            self.results.set_visible_child_name("rich");
            return;
        }
        if q.starts_with("/define") || q.starts_with("/synonym") {
            self.word_header.set_visible(true);
            let key = json!([
                q,
                *self.rows.borrow(),
                value["word_source_counts"],
                *self.settings.borrow()
            ])
            .to_string();
            if *self.word_key.borrow() == key {
                self.results.set_visible_child_name("rich");
                return;
            }
            *self.word_key.borrow_mut() = key;
            let weak = Rc::downgrade(self);
            self.rich.set_child(Some(&views::words(
                &self.rows.borrow(),
                &q,
                &self.settings.borrow(),
                &value["word_source_counts"],
                &self.word_header,
                &self.selection,
                self.hidden_sources.clone(),
                move |word| {
                    if let Some(ui) = weak.upgrade() {
                        ui.lookup(&format!("/define {word}"));
                    }
                },
            )));
            self.results.set_visible_child_name("rich");
            return;
        }
        self.word_key.borrow_mut().clear();
        let capture = [
            "/grab",
            "/shot",
            "/screenshot",
            "/ss",
            "/snap",
            "/rec",
            "/screenrecord",
        ]
        .iter()
        .any(|p| q.trim() == *p);
        if capture {
            if let Some(old) = self.results.child_by_name("capture") {
                self.results.remove(&old);
            }
            let weak = Rc::downgrade(self);
            let menu = views::capture(&self.rows.borrow(), self.selection.selected(), move |at| {
                if let Some(ui) = weak.upgrade() {
                    ui.selection.set_selected(at);
                    ui.activate("default");
                }
            });
            self.columns
                .set(if self.rows.borrow().len() >= 5 { 2 } else { 3 });
            self.results.add_named(&menu, Some("capture"));
            self.results.set_visible_child_name("capture");
            return;
        }
        let browser =
            q == "/browser" || q.starts_with("/browser ") || q == "/zen" || q.starts_with("/zen ");
        let gallery = self.gallery();
        let list = !capture
            && !browser
            && !gallery
            && (*self.view.borrow() == "list"
                || !self.windows_for.borrow().is_empty()
                || (q.starts_with('/') && !q.starts_with("/run") && !q.starts_with("/processes")));
        let cols = if list {
            1
        } else if browser || gallery {
            3
        } else if capture {
            if q == "/rec" {
                2
            } else {
                3
            }
        } else {
            self.settings.borrow()["ui"]["columns"]
                .as_u64()
                .unwrap_or(5) as u32
        }
        .max(1);
        self.columns.set(cols);
        self.grid.set_min_columns(1);
        self.grid.set_max_columns(cols);
        self.grid.set_min_columns(cols);
        let weak = Rc::downgrade(self);
        let icon = self.settings.borrow()["ui"]["icon_size"]
            .as_i64()
            .unwrap_or(52) as i32;
        let key = format!("{gallery}:{icon}:{cols}");
        if *self.factory_key.borrow() != key {
            *self.factory_key.borrow_mut() = key;
            self.grid.set_factory(Some(&results::factory(
                if gallery {
                    Layout::Gallery
                } else {
                    Layout::Grid
                },
                icon,
                move |i| {
                    if let Some(ui) = weak.upgrade() {
                        ui.selection.set_selected(i);
                    }
                },
            )));
        }
        self.results
            .set_visible_child_name(if list { "list" } else { "grid" });
        self.search
            .set_editable(self.windows_for.borrow().is_empty());
    }
    fn desktop_changed(self: &Rc<Self>, event: tjik_desktop::DesktopEvent) {
        if event.sequence <= self.desktop_sequence.get() {
            return;
        }
        self.desktop_sequence.set(event.sequence);
        if self.demo || self.desktop_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(80), move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !ui.opened.get() || ui.stack.visible_child_name().as_deref() == Some("tasks") {
                ui.desktop_pending.set(false);
                return glib::ControlFlow::Break;
            }
            if ui.busy.get() || ui.composing.get() {
                return glib::ControlFlow::Continue;
            }
            ui.desktop_pending.set(false);
            ui.auto.set(false);
            ui.next_generation();
            ui.busy.set(true);
            let entry = ui.windows_for.borrow().clone();
            if entry.is_empty() {
                ui.send(json!({"op":"query","query":ui.query.borrow().as_str(),"limit":ui.rows.borrow().len().max(100)}));
            } else {
                ui.send(json!({"op":"activate","id":entry,"mode":"switch","prepare":true,"observe":true}));
            }
            glib::ControlFlow::Break
        });
    }
    fn schedule_refresh(self: &Rc<Self>, ms: u64) {
        self.cancel_refresh();
        let weak = Rc::downgrade(self);
        *self.refresh.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_millis(ms),
            move || {
                if let Some(ui) = weak.upgrade() {
                    ui.refresh.borrow_mut().take();
                    if ui.opened.get() && !ui.busy.get() && !ui.composing.get() {
                        if ui.stack.visible_child_name().as_deref() == Some("tasks") {
                            ui.tasks.refresh();
                        } else {
                            ui.send(json!({"op":"query","query":ui.query.borrow().as_str(),"limit":ui.rows.borrow().len().max(100)}));
                        }
                    }
                }
            },
        ));
    }
    fn response(self: &Rc<Self>, generation: u64, value: Value) {
        if generation == 0 || s(&value, "kind") == "fatal" {
            self.startup.set_visible(false);
        }
        if s(&value, "kind") == "fatal" {
            *self.error.borrow_mut() = s(&value, "message").into();
            self.ready.set(false);
            self.busy.set(false);
            self.message(s(&value, "message"));
            return;
        }
        if generation == 0 {
            self.configure(&value["config"], true);
            self.ready.set(true);
        }
        if generation != self.generation.get() {
            return;
        }
        self.busy.set(false);
        if value["reset_query"] == true {
            self.set_text("");
        }
        if let Some(query) = value["edit_query"].as_str() {
            self.set_text(query);
        }
        match s(&value, "kind") {
            "results" | "windows" => {
                self.ready.set(true);
                self.configure(&value["config"], value["apply_config"] == true);
                if let Some(history) = value["command_history"].as_array() {
                    *self.history.borrow_mut() = history
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect();
                }
                let query = if s(&value, "kind") == "windows" {
                    self.query.borrow().clone()
                } else {
                    s(&value, "query").to_owned()
                };
                let same = *self.display_query.borrow() == query;
                *self.display_query.borrow_mut() = query;
                *self.windows_for.borrow_mut() = if s(&value, "kind") == "windows" {
                    s(&value, "entry").into()
                } else {
                    String::new()
                };
                let mut rows = value["results"].as_array().cloned().unwrap_or_default();
                for row in &mut rows {
                    if value["closing"]
                        .as_array()
                        .is_some_and(|ids| ids.contains(&row["id"]))
                    {
                        row["closing"] = json!(true);
                    }
                }
                if self.append.replace(false) {
                    let mut old = self.rows.borrow().clone();
                    for row in rows {
                        if !old.iter().any(|r| r["id"] == row["id"]) {
                            old.push(row);
                        }
                    }
                    rows = old;
                }
                self.total
                    .set(value["total"].as_u64().unwrap_or(rows.len() as u64));
                self.indexed
                    .set(value["indexed"].as_u64().unwrap_or(self.indexed.get()));
                self.next_offset
                    .set(value["next_offset"].as_u64().unwrap_or(0));
                self.has_more.set(value["has_more"] == true);
                self.replace_rows(rows, same);
                self.layout(&value);
                self.message(if s(&value, "kind") == "windows" {
                    "Choose a window · Enter switches · Esc goes back"
                } else if !s(&value, "message").is_empty() {
                    s(&value, "message")
                } else if self.rows.borrow().is_empty()
                    && s(&value, "app_mode").is_empty()
                    && value["statistics"].is_null()
                    && value["pending"] != true
                {
                    "No results"
                } else {
                    ""
                });
                if value["auto_select"] == true {
                    self.activate("default");
                } else if let Some(action) = self.pending_action.borrow_mut().take() {
                    self.activate(&action);
                } else if self.auto.replace(false)
                    && self.total.get() == 1
                    && value["pending"] != true
                    && !self.has_more.get()
                {
                    if let Some(row) = self.selected() {
                        let settings = self.settings.borrow();
                        let auto = if s(&row, "provider") == "apps" {
                            settings["ui"]["auto_launch_applications"] != false
                        } else {
                            settings["ui"]["auto_launch_commands"] != false
                                && ["commands", "internal"].contains(&s(&row, "provider"))
                                && !self.query.borrow().contains(char::is_whitespace)
                                && s(&row, "id") != self.query.borrow().trim()
                        };
                        drop(settings);
                        if auto && row["disabled"] != true {
                            self.activate("default");
                        }
                    }
                }
                if value["pending"] == true
                    || value["closing"].as_array().is_some_and(|v| !v.is_empty())
                {
                    self.schedule_refresh(180);
                } else if s(&value, "app_mode") == "Todo" {
                    self.schedule_refresh(1500);
                } else {
                    self.schedule_refresh(
                        self.settings.borrow()["index"]["refresh_seconds"]
                            .as_f64()
                            .unwrap_or(5.) as u64
                            * 1000,
                    );
                }
            }
            "app-state" => {
                self.tasks.update(&value["app_state"]);
                self.message(s(&value["app_state"], "message"));
                self.schedule_refresh(1500);
            }
            "edit" => {
                self.lookup(s(&value, "text"));
                if value["select_value"] == true {
                    let start = self
                        .search
                        .text()
                        .get(8..)
                        .and_then(|t| t.find(' '))
                        .map(|i| (i + 9) as i32)
                        .unwrap_or(0);
                    self.search.select_region(start, -1);
                }
            }
            "launch" => {
                let request = json!({"op":"activate","id":value["entry"],"mode":"launch"});
                if !self.demo {
                    self.window.set_visible(false);
                }
                self.busy.set(true);
                let weak = Rc::downgrade(self);
                glib::timeout_add_local_once(Duration::from_millis(80), move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.send(request);
                    }
                });
            }
            "activated" => {
                if value["action"] == "close" {
                    self.message(s(&value, "message"));
                    self.schedule_refresh(180);
                } else if self.demo {
                    self.message("Demo activation completed; no application was launched.");
                } else if !self.pinned.get() || value["close_launcher"] == true {
                    self.hide();
                } else {
                    self.lookup("");
                }
            }
            "pin" => {
                self.pinned.set(!self.pinned.get());
                self.mark.pinned(self.pinned.get());
                self.watch();
                self.lookup("");
            }
            "view" => {
                *self.view.borrow_mut() = s(&value, "view").into();
                self.lookup("");
            }
            "copied" => {
                self.message("✓ Copied");
                tjik_gtk::motion::reveal(&self.message);
                if !self.demo {
                    let weak = Rc::downgrade(self);
                    glib::timeout_add_local_once(Duration::from_millis(550), move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.hide();
                        }
                    });
                }
            }
            "error" => {
                self.message(s(&value, "message"));
                self.tasks.error(s(&value, "message"));
            }
            _ => {
                self.message(s(&value, "message"));
                if value["recording_stopping"] == true {
                    self.schedule_refresh(150);
                }
            }
        }
        if self.opened.get() && !self.window.is_visible() && !self.busy.get() {
            self.window.present();
            self.watch();
        }
    }
}

fn build(
    application: &gtk::Application,
    send: &async_channel::Sender<Event>,
    demo: bool,
) -> Rc<Ui> {
    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title(if std::env::var_os("RUNTJIK_SESSION").is_some() {
            format!(
                "Runtjik comparison GTK {}",
                std::env::var("RUNTJIK_COMPARE_ID").unwrap_or_default()
            )
        } else {
            "Runtjik".to_owned()
        })
        .default_width(720)
        .default_height(600)
        .decorated(false)
        .build();
    window.add_css_class("tjik");
    let layer = surface::launcher(&window, "runtjik-gtk");
    let root = gtk::Box::new(gtk::Orientation::Vertical, 14);
    root.add_css_class("shell");
    let topbar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let mark = tjik_gtk::mark::Mark::default();
    topbar.append(&mark);
    let search = gtk::Entry::builder()
        .placeholder_text("A beginning is the time for taking the most delicate care…")
        .hexpand(true)
        .build();
    gtk::prelude::EntryExt::set_alignment(&search, 0.0);
    search.set_width_chars(1);
    search.set_max_width_chars(1);
    search.add_css_class("search");
    let click = gtk::GestureClick::new();
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = window.downgrade();
    click.connect_pressed(move |_, _, _, _| {
        if let Some(window) = weak.upgrade() {
            surface::keyboard(&window, true);
        }
    });
    search.add_controller(click);
    topbar.append(&search);
    root.append(&topbar);
    let word_header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    word_header.set_visible(false);
    root.append(&word_header);
    let model = gtk::StringList::new(&[]);
    let selection = gtk::SingleSelection::new(Some(model.clone()));
    let select = selection.clone();
    let list = gtk::ListView::new(
        Some(selection.clone()),
        Some(results::factory(Layout::List, 40, move |i| {
            select.set_selected(i)
        })),
    );
    list.set_single_click_activate(true);
    let select = selection.clone();
    let grid = gtk::GridView::new(
        Some(selection.clone()),
        Some(results::factory(Layout::Grid, 52, move |i| {
            select.set_selected(i)
        })),
    );
    grid.set_single_click_activate(true);
    grid.set_min_columns(5);
    grid.set_max_columns(5);
    let results = gtk::Stack::new();
    results::track_pointer(&results);
    results.set_hhomogeneous(false);
    results.set_vhomogeneous(false);
    results.set_vexpand(true);
    results.add_css_class("pane");
    for (name, widget) in [
        ("list", list.clone().upcast::<gtk::Widget>()),
        ("grid", grid.clone().upcast()),
    ] {
        let scroll = gtk::ScrolledWindow::builder()
            .child(&widget)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        tjik_gtk::scroll::by_row(&scroll, &widget, &selection);
        results.add_named(&scroll, Some(name));
    }
    let rich = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    results.add_named(&rich, Some("rich"));
    results.set_visible_child_name("grid");
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&results));
    overlay.set_vexpand(true);
    let message = gtk::Label::builder()
        .label("")
        .wrap(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    message.add_css_class("card");
    message.add_css_class("accent");
    message.set_can_target(false);
    message.set_visible(false);
    overlay.add_overlay(&message);
    let startup = tjik_gtk::motion::busy_mark(include_bytes!("../../../../docs/icons/runtjik.png"));
    overlay.add_overlay(&startup);
    let stack = gtk::Stack::new();
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_vexpand(true);
    stack.add_named(&overlay, Some("launcher"));
    let events = send.clone();
    let exit = send.clone();
    let tasks = tasktjik_gtk::TaskView::new(
        move |request| {
            let _ = events.try_send(Event::Request(request));
        },
        move || {
            let _ = exit.try_send(Event::ExitView);
        },
        demo,
    );
    stack.add_named(&tasks.root, Some("tasks"));
    stack.set_visible_child_name("launcher");
    root.append(&stack);
    let grip = surface::resize_grip();
    grip.add_css_class("grip");
    surface::resizable(&window, &grip);
    grip.set_halign(gtk::Align::Center);
    grip.set_valign(gtk::Align::End);
    grip.set_margin_bottom(8);
    let frame = gtk::Overlay::new();
    frame.set_child(Some(&root));
    frame.add_overlay(&grip);
    // Bubble phase leaves entries, tiles and buttons in control of their gestures.
    let drag = surface::draggable_with_async_position(&window, &root, desktop::pointer);
    let grab = Rc::new(Cell::new((0., 0.)));
    let point = grab.clone();
    let weak = root.downgrade();
    let target = window.downgrade();
    drag.connect_drag_begin(move |_, x, y| {
        if let (Some(root), Some(window)) = (weak.upgrade(), target.upgrade()) {
            if let Some(p) =
                root.compute_point(&window, &gtk::graphene::Point::new(x as f32, y as f32))
            {
                point.set((p.x() as f64, p.y() as f64));
            }
        }
    });
    let weak = window.downgrade();
    drag.connect_drag_end(move |_, _, _| {
        if let Some(window) = weak.upgrade() {
            desktop::transfer(&window, grab.get());
        }
    });
    window.set_child(Some(&frame));
    let css = theme::provider();
    theme::apply(&css, &Default::default(), "tijpset", 14);
    let pointer = gtk::EventControllerMotion::new();
    window.add_controller(pointer.clone());
    let ui = Rc::new(Ui {
        pointer,
        window,
        search,
        mark,
        topbar,
        model,
        selection,
        list,
        grid,
        stack,
        results,
        rich,
        word_header,
        word_key: RefCell::new(String::new()),
        hidden_sources: Rc::new(RefCell::new(std::collections::HashSet::new())),
        message,
        startup,
        effects: overlay,
        composing: Cell::new(false),
        composition_commit: Cell::new(false),
        tasks,
        css,
        worker: Worker::start(demo, send.clone()),
        rows: RefCell::new(vec![]),
        settings: RefCell::new(json!({})),
        query: RefCell::new(String::new()),
        display_query: RefCell::new(String::new()),
        windows_for: RefCell::new(String::new()),
        generation: Cell::new(0),
        ready: Cell::new(false),
        busy: Cell::new(true),
        changing_text: Cell::new(false),
        debounce: RefCell::new(None),
        refresh: RefCell::new(None),
        error: RefCell::new(String::new()),
        demo,
        last_activation: RefCell::new(String::new()),
        layer,
        pinned: Cell::new(false),
        opened: Cell::new(true),
        restoring: Cell::new(std::env::var_os("RUNTJIK_RESTORE").is_some()),
        desktop_pending: Cell::new(false),
        desktop_sequence: Cell::new(0),
        view: RefCell::new("grid".into()),
        columns: Cell::new(5),
        factory_key: RefCell::new(String::new()),
        total: Cell::new(0),
        next_offset: Cell::new(0),
        has_more: Cell::new(false),
        append: Cell::new(false),
        indexed: Cell::new(0),
        history: RefCell::new(vec![]),
        history_cursor: Cell::new(usize::MAX),
        history_draft: RefCell::new(String::new()),
        auto: Cell::new(false),
        pending_action: RefCell::new(None),
        app: application.clone(),
    });
    // GtkText owns the input-method context; capture shortcuts must defer to it.
    let text = ui
        .search
        .first_child()
        .and_downcast::<gtk::Text>()
        .expect("GtkEntry text");
    let weak = Rc::downgrade(&ui);
    text.connect_preedit_changed(move |_, preedit| {
        let Some(ui) = weak.upgrade() else { return };
        let was_composing = ui.composing.replace(!preedit.is_empty());
        ui.auto.set(false);
        ui.cancel_timer();
        ui.cancel_refresh();
        if !preedit.is_empty() {
            ui.next_generation();
        }
        if preedit.is_empty() && was_composing {
            ui.composition_commit.set(true);
            let weak = Rc::downgrade(&ui);
            glib::idle_add_local_once(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.composition_commit.set(false);
                    if !ui.composing.get() {
                        ui.query(true);
                    }
                }
            });
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.search.connect_changed(move |entry| {
        if let Some(ui) = weak.upgrade() {
            if !ui.changing_text.get() && !ui.composing.get() && !ui.composition_commit.get() {
                ui.auto.set(entry.text().len() > ui.query.borrow().len());
                ui.history_cursor.set(usize::MAX);
                ui.query(true);
            }
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.search.connect_activate(move |_| {
        if let Some(ui) = weak.upgrade() {
            if ui.composing.get() || ui.composition_commit.get() {
                return;
            }
            ui.activate("default");
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.grid.connect_activate(move |_, i| {
        if let Some(ui) = weak.upgrade() {
            ui.selection.set_selected(i);
            ui.activate("default");
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.list.connect_activate(move |_, i| {
        if let Some(ui) = weak.upgrade() {
            ui.selection.set_selected(i);
            ui.activate("default");
        }
    });
    for name in ["list", "grid"] {
        let scroll = ui
            .results
            .child_by_name(name)
            .unwrap()
            .downcast::<gtk::ScrolledWindow>()
            .unwrap();
        let weak = Rc::downgrade(&ui);
        scroll.vadjustment().connect_value_changed(move |a| {
            if a.value() + a.page_size() >= a.upper() - 180. {
                if let Some(ui) = weak.upgrade() {
                    ui.load_more();
                }
            }
        });
    }
    let weak = Rc::downgrade(&ui);
    ui.selection.connect_selected_notify(move |selection| {
        if let Some(ui) = weak.upgrade() {
            let at = selection.selected();
            if at != gtk::INVALID_LIST_POSITION {
                views::select_word(&ui.results, at);
            }
        }
    });
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(&ui);
    keys.connect_key_pressed(move |_, key, _, mods| {
        let Some(ui) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        if ui.composing.get() || ui.composition_commit.get() {
            return glib::Propagation::Proceed;
        }
        if ui.stack.visible_child_name().as_deref() == Some("tasks") {
            return glib::Propagation::Proceed;
        }
        if ui.results.visible_child_name().as_deref() == Some("rich") && key != gdk::Key::Escape {
            if ui.word_header.is_visible()
                && matches!(
                    key,
                    gdk::Key::Up | gdk::Key::Down | gdk::Key::Tab | gdk::Key::ISO_Left_Tab
                )
            {
                ui.move_selection(
                    if key == gdk::Key::Down
                        || (key == gdk::Key::Tab && !mods.contains(gdk::ModifierType::SHIFT_MASK))
                    {
                        1
                    } else {
                        -1
                    },
                );
                return glib::Propagation::Stop;
            }
            return glib::Propagation::Proceed;
        }
        let list = matches!(
            ui.results.visible_child_name().as_deref(),
            Some("list") | Some("rich")
        );
        match key {
            gdk::Key::Escape => {
                if ui.query.borrow().starts_with('/')
                    || !ui.windows_for.borrow().is_empty()
                    || ui.query.borrow().starts_with('?')
                {
                    ui.lookup("");
                } else {
                    ui.hide();
                }
            }
            gdk::Key::Down | gdk::Key::Up if mods.contains(gdk::ModifierType::CONTROL_MASK) => {
                ui.browse_history(if key == gdk::Key::Up { -1 } else { 1 })
            }
            gdk::Key::Down => ui.move_selection(if list { 1 } else { ui.columns.get() as i32 }),
            gdk::Key::Up => ui.move_selection(if list { -1 } else { -(ui.columns.get() as i32) }),
            gdk::Key::Left | gdk::Key::Right
                if !list || mods.contains(gdk::ModifierType::CONTROL_MASK) =>
            {
                ui.move_selection(if key == gdk::Key::Left { -1 } else { 1 })
            }
            gdk::Key::Tab | gdk::Key::ISO_Left_Tab => {
                ui.move_selection(if mods.contains(gdk::ModifierType::SHIFT_MASK) {
                    -1
                } else {
                    1
                })
            }
            gdk::Key::l if mods.contains(gdk::ModifierType::CONTROL_MASK) => {
                let grid = *ui.view.borrow() == "grid";
                *ui.view.borrow_mut() = if grid { "list" } else { "grid" }.into();
                ui.layout(&json!({}));
            }
            gdk::Key::z if mods.contains(gdk::ModifierType::CONTROL_MASK) && ui.gallery() => {
                ui.next_generation();
                ui.send(json!({"op":"undo-trash","query":ui.query.borrow().as_str()}));
            }
            _ => return glib::Propagation::Proceed,
        };
        glib::Propagation::Stop
    });
    ui.window.add_controller(keys);
    let modified = gtk::EventControllerKey::new();
    modified.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(&ui);
    modified.connect_key_pressed(move |_, key, _, mods| {
        let Some(ui) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        if ui.composing.get() || ui.composition_commit.get() {
            return glib::Propagation::Proceed;
        }
        if ui.stack.visible_child_name().as_deref() == Some("tasks") {
            return glib::Propagation::Proceed;
        }
        if key == gdk::Key::c
            && mods.contains(gdk::ModifierType::CONTROL_MASK)
            && ui.gallery()
            && ui.search.selection_bounds().is_none()
        {
            ui.activate("copy");
            return glib::Propagation::Stop;
        }
        if ![gdk::Key::Return, gdk::Key::KP_Enter].contains(&key) {
            return glib::Propagation::Proceed;
        }
        if mods.contains(gdk::ModifierType::CONTROL_MASK) {
            ui.activate("close");
        } else if mods.contains(gdk::ModifierType::SHIFT_MASK) {
            ui.activate("alternate");
        } else {
            return glib::Propagation::Proceed;
        }
        glib::Propagation::Stop
    });
    ui.window.add_controller(modified);
    let weak = Rc::downgrade(&ui);
    ui.window.connect_close_request(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.hide();
        }
        glib::Propagation::Stop
    });
    let weak = Rc::downgrade(&ui);
    ui.window.connect_map(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.watch();
        }
    });
    let restore_hidden = std::env::var("RUNTJIK_RESTORE")
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .is_some_and(|s| s["visible"] == false);
    if restore_hidden {
        ui.opened.set(false);
    } else {
        ui.show();
    }
    ui
}
pub fn run(demo: bool) -> Result<()> {
    let app = gtk::Application::builder()
        .application_id("org.tjikos.Runtjik")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let (send, receive) = async_channel::bounded::<Event>(64);
    let _listener = control::listen(demo, send.clone())?;
    desktop::events(send.clone());
    let owner: Rc<RefCell<Option<Rc<Ui>>>> = Rc::new(RefCell::new(None));
    let activation_owner = owner.clone();
    app.connect_activate(move |app| {
        if let Some(ui) = activation_owner.borrow().as_ref() {
            ui.show();
            return;
        }
        let ui = build(app, &send, demo);
        let weak = Rc::downgrade(&ui);
        let receiver = receive.clone();
        glib::spawn_future_local(async move {
            while let Ok(event) = receiver.recv().await {
                let Some(ui) = weak.upgrade() else { break };
                match event {
                    Event::Quit => ui.app.quit(),
                    Event::Desktop(event) => ui.desktop_changed(event),
                    Event::Response(generation, value) => {
                        ui.response(generation, value);
                        if generation == 0 && ui.restoring.replace(false) {
                            if let Ok(saved) = std::env::var("RUNTJIK_RESTORE").and_then(|s| {
                                serde_json::from_str::<Value>(&s)
                                    .map_err(|_| std::env::VarError::NotPresent)
                            }) {
                                ui.auto.set(false);
                                ui.set_text(s(&saved, "query"));
                                ui.query(false);
                                ui.pinned.set(saved["pinned"] == true);
                                ui.mark.pinned(ui.pinned.get());
                                if let Some(view) = saved["view"].as_str() {
                                    *ui.view.borrow_mut() = view.into();
                                }
                                if saved["visible"] == false {
                                    ui.opened.set(false);
                                    ui.window.set_visible(false);
                                    surface::keyboard(&ui.window, false);
                                    ui.cancel_refresh();
                                } else {
                                    ui.watch();
                                }
                                ui.recovery_state();
                            }
                        }
                    }
                    Event::ExitView => {
                        ui.topbar.set_visible(true);
                        ui.stack.set_visible_child_name("launcher");
                        ui.lookup("");
                    }
                    Event::Request(value) => {
                        if value["op"] == "gtk-open-task-window" {
                            if !ui.demo {
                                match std::process::Command::new("tasktjik")
                                    .arg("show")
                                    .env("TASKTJIK_COLLECTION", s(&value, "collection"))
                                    .env("TASKTJIK_VIEW", "all")
                                    .spawn()
                                {
                                    Ok(_) => ui.hide(),
                                    Err(e) => ui.message(&format!("Could not open Tasktjik: {e}")),
                                }
                            }
                            continue;
                        }
                        if ui.busy.get() {
                            ui.tasks
                                .error("A request is still running. Try again when it finishes.");
                        } else {
                            ui.next_generation();
                            ui.send(value);
                        }
                    }
                    Event::Control(command, reply) => {
                        let mut result = json!({"ok":true});
                        match s(&command, "action") {
                            "status" => result = ui.status(),
                            "show" => ui.show(),
                            "toggle" => {
                                if ui.opened.get() {
                                    ui.hide();
                                } else {
                                    ui.show();
                                }
                            }
                            "close-target" => {
                                let handled = ui.window.is_visible()
                                    && (ui.window.is_active() || ui.pointer.contains_pointer());
                                if handled {
                                    ui.hide();
                                }
                                result = json!({"handled":handled});
                            }
                            "close" => ui.hide(),
                            "outside-click" => ui.outside(),
                            "quit" => {
                                ui.cancel_timer();
                                ui.cancel_refresh();
                            }
                            "test-query" if demo => ui.lookup(s(&command, "text")),
                            "test-activate" if demo => ui.activate(s(&command, "text")),
                            "test-response" if demo => {
                                if let Ok(value) = serde_json::from_str(s(&command, "text")) {
                                    ui.response(ui.generation.get(), value);
                                }
                            }
                            "test-request" if demo => {
                                match serde_json::from_str::<Value>(s(&command, "text")) {
                                    Ok(request) => {
                                        ui.next_generation();
                                        ui.send(request);
                                    }
                                    Err(e) => result = json!({"error":e.to_string()}),
                                }
                            }
                            _ => {
                                result =
                                    json!({"error":"Unknown action (test actions require --demo)"})
                            }
                        }
                        let _ = reply.send(result);
                    }
                }
            }
        });
        *activation_owner.borrow_mut() = Some(ui);
    });
    app.run_with_args(&["runtjik-gtk"]);
    owner.borrow_mut().take();
    let _ = std::fs::remove_file(control::socket(demo)?);
    Ok(())
}
