//! Reading presentation shared by embedded and standalone Tasktjik views.
use super::{button, clear, s};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use tjik_gtk::gtk::{self, gio, glib, prelude::*};
pub struct Reading {
    cards: RefCell<Vec<(String, gtk::Widget)>>,
    demo: bool,
    pub root: gtk::Box,
    body: gtk::Box,
    count: gtk::Label,
    filter: gtk::SearchEntry,
    grid: gtk::ToggleButton,
    categories: gtk::ToggleButton,
    completed: gtk::ToggleButton,
    sort: gtk::DropDown,
    state: RefCell<Value>,
    selected: RefCell<String>,
    previews: RefCell<HashMap<String, Value>>,
    send: RefCell<Option<Rc<dyn Fn(&str, Value)>>>,
}
impl Reading {
    pub fn new(demo: bool) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 16);
        root.set_vexpand(true);
        root.set_focusable(true);
        root.add_css_class("reading");
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let heading = gtk::Box::new(gtk::Orientation::Vertical, 4);
        heading.set_hexpand(true);
        let title = gtk::Label::new(Some("Reading"));
        title.set_xalign(0.);
        title.add_css_class("reading-heading");
        heading.append(&title);
        let count = gtk::Label::new(Some("0 saved pages"));
        count.set_xalign(0.);
        count.add_css_class("muted");
        heading.append(&count);
        header.append(&heading);
        let options = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let list = gtk::ToggleButton::with_label("List");
        list.set_active(true);
        let grid = gtk::ToggleButton::with_label("Grid");
        grid.set_group(Some(&list));
        options.append(&list);
        options.append(&grid);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        options.append(&spacer);
        let sort = gtk::DropDown::from_strings(&["Recently saved", "Title: A–Z", "Site: A–Z"]);
        options.append(&sort);
        let search = gtk::ToggleButton::with_label("⌕");
        search.set_tooltip_text(Some("Filter saved pages"));
        options.append(&search);
        let completed = gtk::ToggleButton::with_label("✓");
        completed.set_tooltip_text(Some("Show read pages"));
        options.append(&completed);
        let categories = gtk::ToggleButton::with_label("☷");
        categories.set_tooltip_text(Some("Group by category"));
        options.append(&categories);
        root.append(&header);
        root.append(&options);
        let filter = gtk::SearchEntry::new();
        filter.set_visible(false);
        root.append(&filter);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&body)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        root.append(&scroll);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.append(&footer);
        let v = Rc::new(Self {
            cards: RefCell::new(vec![]),
            demo,
            root,
            body,
            count,
            filter,
            grid,
            categories,
            completed,
            sort,
            state: RefCell::new(json!({})),
            selected: RefCell::new(String::new()),
            previews: RefCell::new(HashMap::new()),
            send: RefCell::new(None),
        });
        for (title, action) in [
            ("Reading", "collection"),
            ("Open window", "window"),
            ("+ Save page", "new"),
        ] {
            let weak = Rc::downgrade(&v);
            header.append(&button(title, move || {
                if let Some(v) = weak.upgrade() {
                    v.emit(action, json!({}));
                }
            }));
        }
        for (title, action) in [
            ("‹ Launcher", "exit"),
            ("Edit", "edit"),
            ("Mark read", "toggle"),
            ("Undo", "undo"),
        ] {
            let weak = Rc::downgrade(&v);
            footer.append(&button(title, move || {
                if let Some(v) = weak.upgrade() {
                    v.emit_selected(action);
                }
            }));
        }
        for toggle in [&v.grid, &v.categories, &v.completed] {
            let weak = Rc::downgrade(&v);
            toggle.connect_toggled(move |_| {
                if let Some(v) = weak.upgrade() {
                    v.render();
                }
            });
        }
        let weak = Rc::downgrade(&v);
        v.sort.connect_selected_notify(move |_| {
            if let Some(v) = weak.upgrade() {
                v.render();
            }
        });
        let weak = Rc::downgrade(&v);
        v.filter.connect_search_changed(move |_| {
            if let Some(v) = weak.upgrade() {
                v.render();
            }
        });
        let weak = Rc::downgrade(&v);
        search.connect_toggled(move |t| {
            if let Some(v) = weak.upgrade() {
                v.filter.set_visible(t.is_active());
                if t.is_active() {
                    v.filter.grab_focus();
                } else {
                    v.filter.set_text("");
                }
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&v);
        keys.connect_key_pressed(move |_, key, _, mods| {
            let Some(v) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if v.filter.has_focus() {
                return glib::Propagation::Proceed;
            }
            match key {
                gtk::gdk::Key::Return => v.emit_selected("open-reference"),
                gtk::gdk::Key::n if mods.contains(gtk::gdk::ModifierType::CONTROL_MASK) => {
                    v.emit("new", json!({}))
                }
                gtk::gdk::Key::slash => {
                    v.filter.set_visible(true);
                    v.filter.grab_focus();
                }
                gtk::gdk::Key::Escape => v.emit("exit", json!({})),
                gtk::gdk::Key::Down
                | gtk::gdk::Key::Up
                | gtk::gdk::Key::Left
                | gtk::gdk::Key::Right => {
                    let cards = v.cards.borrow();
                    if cards.is_empty() {
                        return glib::Propagation::Stop;
                    }
                    let at = cards
                        .iter()
                        .position(|(id, _)| *id == *v.selected.borrow())
                        .unwrap_or(0);
                    let delta = if matches!(key, gtk::gdk::Key::Up | gtk::gdk::Key::Left) {
                        -1
                    } else {
                        1
                    };
                    let next = (at as i32 + delta).clamp(0, cards.len() as i32 - 1) as usize;
                    for (_, card) in cards.iter() {
                        card.remove_css_class("task-selected");
                    }
                    *v.selected.borrow_mut() = cards[next].0.clone();
                    cards[next].1.add_css_class("task-selected");
                    cards[next].1.grab_focus();
                }
                _ => return glib::Propagation::Proceed,
            };
            glib::Propagation::Stop
        });
        v.root.add_controller(keys);
        v
    }
    pub fn connect(&self, send: impl Fn(&str, Value) + 'static) {
        *self.send.borrow_mut() = Some(Rc::new(send));
    }
    fn emit(&self, action: &str, task: Value) {
        if let Some(send) = self.send.borrow().as_ref() {
            send(action, task);
        }
    }
    fn emit_selected(&self, action: &str) {
        let task = self.state.borrow()["tasks"]
            .as_array()
            .and_then(|ts| ts.iter().find(|t| s(t, "id") == *self.selected.borrow()))
            .cloned()
            .unwrap_or(json!({}));
        self.emit(action, task);
    }
    pub fn update(self: &Rc<Self>, state: &Value) {
        *self.state.borrow_mut() = state.clone();
        self.render();
        self.previews();
    }
    fn previews(self: &Rc<Self>) {
        let urls: Vec<String> = self.state.borrow()["tasks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| s(t, "kind") == "reading")
            .map(|t| s(t, "reference").to_owned())
            .filter(|u| {
                (u.starts_with("https://") || u.starts_with("http://"))
                    && !self.previews.borrow().contains_key(u)
            })
            .collect();
        if urls.is_empty() {
            return;
        }
        for url in &urls {
            self.previews.borrow_mut().insert(url.clone(), json!({}));
        }
        // Reuse the original preview service and cache; only saved references
        // enter this queue. Synthetic fixtures never visit external pages.
        if self.demo {
            return;
        }
        let helper = std::env::var("RUNTJIK_HOME")
            .map(|h| format!("{h}/qml/page_preview.py"))
            .unwrap_or_default();
        if !std::path::Path::new(&helper).is_file() {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let args: Vec<&std::ffi::OsStr> = ["python3", "-B", &helper, "--batch"]
                .into_iter()
                .map(std::ffi::OsStr::new)
                .collect();
            let Ok(p) = gio::Subprocess::newv(
                &args,
                gio::SubprocessFlags::STDIN_PIPE
                    | gio::SubprocessFlags::STDOUT_PIPE
                    | gio::SubprocessFlags::STDERR_SILENCE,
            ) else {
                return;
            };
            let input = glib::Bytes::from_owned(serde_json::to_vec(&urls).unwrap());
            if let Ok((Some(output), _)) = p.communicate_future(Some(&input)).await {
                if let Some(v) = weak.upgrade() {
                    for line in String::from_utf8_lossy(&output).lines() {
                        if let Ok(value) = serde_json::from_str::<Value>(line) {
                            v.previews
                                .borrow_mut()
                                .insert(s(&value, "url").into(), value["preview"].clone());
                        }
                    }
                    v.render();
                }
            }
        });
    }
    fn render(self: &Rc<Self>) {
        self.cards.borrow_mut().clear();
        clear(&self.body);
        let state = self.state.borrow();
        let query = self.filter.text().to_lowercase();
        let mut sites: Vec<Value> = state["tasks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| {
                s(t, "kind") == "reading"
                    && s(t, "status") != "cancelled"
                    && (self.completed.is_active() || t["done"] != true)
                    && format!("{} {} {}", s(t, "title"), s(t, "reference"), t["tags"])
                        .to_lowercase()
                        .contains(&query)
            })
            .cloned()
            .collect();
        sites.sort_by(|a, b| match self.sort.selected() {
            1 => s(a, "title").cmp(s(b, "title")),
            2 => s(a, "reference").cmp(s(b, "reference")),
            _ => s(b, "createdAt").cmp(s(a, "createdAt")),
        });
        self.count.set_text(&format!("{} saved pages", sites.len()));
        if !sites.iter().any(|t| s(t, "id") == *self.selected.borrow()) {
            *self.selected.borrow_mut() = sites
                .first()
                .map(|t| s(t, "id").to_owned())
                .unwrap_or_default();
        }
        let mut groups = vec![String::new()];
        if self.categories.is_active() {
            groups = sites.iter().map(|t| s(t, "group").to_owned()).collect();
            groups.sort();
            groups.dedup();
        }
        if sites.is_empty() {
            self.body.append(&gtk::Label::new(Some(
                "No pages here yet\nSave a page or adjust your filters.",
            )));
        }
        for group in groups {
            if self.categories.is_active() {
                let l = gtk::Label::new(Some(if group == "All" { "Inbox" } else { &group }));
                l.set_xalign(0.);
                l.add_css_class("muted");
                self.body.append(&l);
            }
            let flow = gtk::FlowBox::new();
            flow.set_selection_mode(gtk::SelectionMode::None);
            flow.set_homogeneous(true);
            flow.set_min_children_per_line(1);
            flow.set_max_children_per_line(if self.grid.is_active() { 3 } else { 1 });
            flow.set_row_spacing(10);
            flow.set_column_spacing(10);
            self.body.append(&flow);
            for task in sites
                .iter()
                .filter(|t| !self.categories.is_active() || s(t, "group") == group)
            {
                let vertical = self.grid.is_active();
                let card = gtk::Box::new(
                    if vertical {
                        gtk::Orientation::Vertical
                    } else {
                        gtk::Orientation::Horizontal
                    },
                    14,
                );
                card.add_css_class("reading-card");
                card.set_focusable(true);
                card.set_hexpand(true);
                if s(task, "id") == *self.selected.borrow() {
                    card.add_css_class("task-selected");
                }
                let previews = self.previews.borrow();
                let preview = previews
                    .get(s(task, "reference"))
                    .cloned()
                    .unwrap_or(json!({}));
                drop(previews);
                let url = s(task, "reference");
                let domain = url
                    .split("://")
                    .nth(1)
                    .unwrap_or(url)
                    .split('/')
                    .next()
                    .unwrap_or("")
                    .trim_start_matches("www.");
                let art = gtk::Overlay::new();
                art.add_css_class("reading-art");
                art.set_overflow(gtk::Overflow::Hidden);
                art.set_size_request(if vertical { 240 } else { 156 }, 126);
                let initial = gtk::Label::new(
                    domain
                        .chars()
                        .next()
                        .map(|c| c.to_uppercase().to_string())
                        .as_deref(),
                );
                initial.add_css_class("reading-heading");
                art.set_child(Some(&initial));
                if s(&preview, "image").starts_with("file://") {
                    let picture = gtk::Picture::for_file(&gio::File::for_uri(s(&preview, "image")));
                    picture.set_content_fit(gtk::ContentFit::Cover);
                    picture.set_can_shrink(true);
                    art.add_overlay(&picture);
                }
                card.append(&art);
                let details = gtk::Box::new(gtk::Orientation::Vertical, 6);
                details.set_hexpand(true);
                for (text, class) in [
                    (domain, "accent"),
                    (s(task, "title"), "reading-title"),
                    (
                        if s(&preview, "description").is_empty() {
                            url.split_once(domain).map(|(_, path)| path).unwrap_or("/")
                        } else {
                            s(&preview, "description")
                        },
                        "muted",
                    ),
                ] {
                    let l = gtk::Label::new(Some(text));
                    l.set_xalign(0.);
                    l.set_wrap(true);
                    l.set_lines(2);
                    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    l.add_css_class(class);
                    details.append(&l);
                }
                let foot = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                foot.set_vexpand(true);
                foot.set_valign(gtk::Align::End);
                let tags = task["tags"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|t| format!("#{t}"))
                    .collect::<Vec<_>>()
                    .join("  ");
                let tags = gtk::Label::new(Some(&tags));
                tags.set_hexpand(true);
                tags.set_xalign(0.);
                tags.add_css_class("muted");
                foot.append(&tags);
                let weak = Rc::downgrade(self);
                let item = task.clone();
                foot.append(&button("Open page ↗", move || {
                    if let Some(v) = weak.upgrade() {
                        v.emit("open-reference", item.clone());
                    }
                }));
                details.append(&foot);
                card.append(&details);
                let weak = Rc::downgrade(self);
                let item = task.clone();
                let click = gtk::GestureClick::new();
                click.connect_released(move |_, n, _, _| {
                    if let Some(v) = weak.upgrade() {
                        *v.selected.borrow_mut() = s(&item, "id").into();
                        if n == 2 {
                            v.emit("open-reference", item.clone());
                        } else {
                            for (id, card) in v.cards.borrow().iter() {
                                if id == s(&item, "id") {
                                    card.add_css_class("task-selected");
                                } else {
                                    card.remove_css_class("task-selected");
                                }
                            }
                        }
                    }
                });
                card.add_controller(click);
                self.cards
                    .borrow_mut()
                    .push((s(task, "id").to_owned(), card.clone().upcast()));
                flow.insert(&card, -1);
            }
        }
    }
}
