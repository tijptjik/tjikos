//! Reading presentation shared by embedded and standalone Tasktjik views.
use super::{
    button,
    previews::{PagePreview, PreviewProvider},
    s,
};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    rc::Rc,
};
use tjik_gtk::gtk::{self, gio, glib, prelude::*};

enum Row {
    Heading(String),
    Cards(Vec<Value>, usize),
    Empty,
}
pub struct Reading {
    cards: RefCell<Vec<(String, gtk::Widget)>>,
    preview_widgets:
        RefCell<HashMap<String, Vec<(glib::WeakRef<gtk::Overlay>, glib::WeakRef<gtk::Label>)>>>,
    provider: Option<Rc<dyn PreviewProvider>>,
    request: RefCell<Option<glib::JoinHandle<()>>>,
    pending_previews: RefCell<VecDeque<String>>,
    pub root: gtk::Box,
    body: gtk::ListView,
    model: gio::ListStore,
    viewport: super::viewport::Viewport,
    columns: Cell<usize>,
    order: RefCell<Vec<(String, u32)>>,
    count: gtk::Label,
    filter: gtk::SearchEntry,
    grid: gtk::ToggleButton,
    categories: gtk::ToggleButton,
    completed: super::status_filter::StatusFilter,
    sort: gtk::DropDown,
    state: RefCell<Value>,
    selected: RefCell<String>,
    previews: RefCell<HashMap<String, PagePreview>>,
    send: RefCell<Option<Rc<dyn Fn(&str, Value)>>>,
}
impl Reading {
    pub fn new(provider: Option<Rc<dyn PreviewProvider>>) -> Rc<Self> {
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
        let sort = gtk::DropDown::from_strings(&[
            "Recently saved",
            "Title: A–Z",
            "Site: A–Z",
            "Recently completed",
        ]);
        options.append(&sort);
        let search = gtk::ToggleButton::with_label("⌕");
        search.set_child(Some(&super::icons::icon("search")));
        search.add_css_class("icon-button");
        search.set_tooltip_text(Some("Filter saved pages"));
        options.append(&search);
        let completed = super::status_filter::StatusFilter::new("Read pages", "completed");
        options.append(&completed.button);
        let categories = gtk::ToggleButton::with_label("☷");
        categories.set_tooltip_text(Some("Group by category"));
        options.append(&categories);
        root.append(&header);
        root.append(&options);
        let filter = gtk::SearchEntry::new();
        filter.set_visible(false);
        root.append(&filter);
        let model = gio::ListStore::new::<glib::BoxedAnyObject>();
        let factory = gtk::SignalListItemFactory::new();
        let body = gtk::ListView::new(
            Some(gtk::NoSelection::new(Some(model.clone()))),
            Some(factory.clone()),
        );
        body.set_single_click_activate(false);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&body)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let viewport = super::viewport::Viewport::new(&scroll);
        root.append(&viewport);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.append(&footer);
        let v = Rc::new(Self {
            cards: RefCell::new(vec![]),
            preview_widgets: RefCell::new(HashMap::new()),
            provider,
            request: RefCell::new(None),
            pending_previews: RefCell::new(VecDeque::new()),
            root,
            body,
            model,
            viewport,
            columns: Cell::new(1),
            order: RefCell::new(Vec::new()),
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
        let weak = Rc::downgrade(&v);
        factory.connect_bind(move |_, object| {
            let Some(view) = weak.upgrade() else { return };
            let item = object.downcast_ref::<gtk::ListItem>().unwrap();
            item.set_selectable(false);
            let model = item.item().and_downcast::<glib::BoxedAnyObject>().unwrap();
            item.set_child(Some(&view.row(&model.borrow::<Row>())));
        });
        let weak = Rc::downgrade(&v);
        factory.connect_unbind(move |_, object| {
            let item = object.downcast_ref::<gtk::ListItem>().unwrap();
            if let (Some(view), Some(row)) = (weak.upgrade(), item.child()) {
                view.cards
                    .borrow_mut()
                    .retain(|(_, card)| !card.is_ancestor(&row));
                view.preview_widgets.borrow_mut().retain(|_, widgets| {
                    widgets
                        .retain(|(art, _)| art.upgrade().is_some_and(|art| !art.is_ancestor(&row)));
                    !widgets.is_empty()
                });
            }
            item.set_child(None::<&gtk::Widget>);
        });
        let weak = Rc::downgrade(&v);
        v.viewport.connect_width(move |_| {
            if let Some(view) = weak.upgrade() {
                if view.columns.get() != view.column_count() {
                    view.render();
                }
            }
        });
        for (title, action) in [("Todo", "collection"), ("+ Save page", "new")] {
            let weak = Rc::downgrade(&v);
            let control = button(title, move || {
                if let Some(v) = weak.upgrade() {
                    v.emit(action, json!({}));
                }
            });
            if action == "new" {
                control.add_css_class("primary");
            }
            header.append(&control);
        }
        for (title, action) in [("Edit", "edit"), ("Mark read", "toggle"), ("Undo", "undo")] {
            let weak = Rc::downgrade(&v);
            footer.append(&button(title, move || {
                if let Some(v) = weak.upgrade() {
                    v.emit_selected(action);
                }
            }));
        }
        let weak = Rc::downgrade(&v);
        v.completed.button.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                v.render();
            }
        });
        for toggle in [&v.grid, &v.categories] {
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
                if v.sort.selected() == 3 && v.completed.mode() == 0 {
                    v.completed.set_mode(1);
                }
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
        keys.connect_key_pressed(move |_, key, _, _mods| {
            let Some(v) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if super::focus_within(&v.filter) {
                if key == gtk::gdk::Key::Escape {
                    v.filter.set_text("");
                    v.filter.set_visible(false);
                    v.root.grab_focus();
                    return glib::Propagation::Stop;
                }
                return glib::Propagation::Proceed;
            }
            match key {
                gtk::gdk::Key::Return => v.emit_selected("open-reference"),
                gtk::gdk::Key::n | gtk::gdk::Key::N => v.emit("new", json!({})),
                gtk::gdk::Key::slash => {
                    v.filter.set_visible(true);
                    v.filter.grab_focus();
                }
                gtk::gdk::Key::e => v.emit_selected("edit"),
                gtk::gdk::Key::Escape => v.emit("exit", json!({})),
                gtk::gdk::Key::Down
                | gtk::gdk::Key::Up
                | gtk::gdk::Key::Left
                | gtk::gdk::Key::Right => {
                    let order = v.order.borrow();
                    if order.is_empty() {
                        return glib::Propagation::Stop;
                    }
                    let at = order
                        .iter()
                        .position(|(id, _)| *id == *v.selected.borrow())
                        .unwrap_or(0);
                    let delta = if matches!(key, gtk::gdk::Key::Up | gtk::gdk::Key::Left) {
                        -1
                    } else {
                        1
                    };
                    let next = (at as i32 + delta).clamp(0, order.len() as i32 - 1) as usize;
                    drop(order);
                    v.select_index(next);
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
        self.queue_previews();
        self.previews();
    }
    fn queue_previews(&self) {
        if self.provider.is_none() {
            return;
        }
        let references: std::collections::HashSet<String> = self.state.borrow()["tasks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| s(t, "kind") == "reading")
            .map(|t| s(t, "reference").to_owned())
            .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
            .collect();
        self.previews
            .borrow_mut()
            .retain(|url, _| references.contains(url));
        *self.pending_previews.borrow_mut() = references
            .into_iter()
            .filter(|u| !self.previews.borrow().contains_key(u))
            .collect();
    }
    fn previews(self: &Rc<Self>) {
        let Some(provider) = &self.provider else {
            return;
        };
        if self.request.borrow().is_some() {
            return;
        }
        let urls: Vec<String> = {
            let mut pending = self.pending_previews.borrow_mut();
            let count = pending.len().min(4);
            pending.drain(..count).collect()
        };
        if urls.is_empty() {
            return;
        }
        for url in &urls {
            self.previews
                .borrow_mut()
                .insert(url.clone(), PagePreview::default());
        }
        let future = provider.load(urls.clone());
        let weak = Rc::downgrade(self);
        *self.request.borrow_mut() = Some(glib::MainContext::default().spawn_local(async move {
            let results = future.await;
            if let Some(v) = weak.upgrade() {
                for preview in results {
                    if urls.contains(&preview.url) && v.previews.borrow().contains_key(&preview.url)
                    {
                        v.refresh_preview(&preview);
                        v.previews.borrow_mut().insert(preview.url.clone(), preview);
                    }
                }
                v.request.borrow_mut().take();
                v.previews();
            }
        }));
    }
    fn refresh_preview(&self, preview: &PagePreview) {
        if let Some(widgets) = self.preview_widgets.borrow().get(&preview.url) {
            for (art, description) in widgets {
                if let Some(label) = description.upgrade() {
                    if !preview.description.is_empty() {
                        label.set_text(&preview.description);
                    }
                }
                if let Some(art) = art.upgrade() {
                    if preview.image.starts_with("file://") {
                        let picture = gtk::Picture::new();
                        tjik_gtk::images::set_picture(&picture, &preview.image);
                        picture.set_content_fit(gtk::ContentFit::Cover);
                        picture.set_can_shrink(true);
                        art.add_overlay(&picture);
                    }
                }
            }
        }
    }
    fn render(self: &Rc<Self>) {
        let anchor = self.visible_anchor();
        self.cards.borrow_mut().clear();
        self.preview_widgets.borrow_mut().clear();
        self.columns.set(self.column_count());
        let state = self.state.borrow();
        let query = self.filter.text().to_lowercase();
        let mut sites: Vec<Value> = state["tasks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| {
                s(t, "kind") == "reading"
                    && s(t, "status") != "cancelled"
                    && self.completed.accepts(t["done"] == true)
                    && format!("{} {} {}", s(t, "title"), s(t, "reference"), t["tags"])
                        .to_lowercase()
                        .contains(&query)
            })
            .cloned()
            .collect();
        sites.sort_by(|a, b| match self.sort.selected() {
            1 => s(a, "title").cmp(s(b, "title")),
            2 => s(a, "reference").cmp(s(b, "reference")),
            3 => (b["done"] == true).cmp(&(a["done"] == true)).then_with(|| {
                super::presentation::completion_time(b)
                    .cmp(&super::presentation::completion_time(a))
            }),
            _ => s(b, "createdAt").cmp(s(a, "createdAt")),
        });
        self.count.set_text(&format!("{} saved pages", sites.len()));
        if !sites.iter().any(|t| s(t, "id") == *self.selected.borrow()) {
            *self.selected.borrow_mut() = sites
                .first()
                .map(|t| s(t, "id").to_owned())
                .unwrap_or_default();
        }
        let mut grouped = std::collections::BTreeMap::<String, Vec<Value>>::new();
        for task in sites {
            let group = if self.categories.is_active() {
                s(&task, "group").to_owned()
            } else {
                String::new()
            };
            grouped.entry(group).or_default().push(task);
        }
        let mut rows = Vec::new();
        let mut order = Vec::new();
        if grouped.is_empty() {
            rows.push(glib::BoxedAnyObject::new(Row::Empty));
        }
        for (group, tasks) in grouped {
            if self.categories.is_active() {
                rows.push(glib::BoxedAnyObject::new(Row::Heading(group)));
            }
            let columns = self.columns.get().min(tasks.len()).max(1);
            for chunk in tasks.chunks(columns) {
                let index = rows.len() as u32;
                order.extend(chunk.iter().map(|task| (s(task, "id").to_owned(), index)));
                rows.push(glib::BoxedAnyObject::new(Row::Cards(
                    chunk.to_vec(),
                    columns,
                )));
            }
        }
        *self.order.borrow_mut() = order;
        self.model.splice(0, self.model.n_items(), &rows);
        if let Some(index) = anchor.and_then(|id| {
            self.order
                .borrow()
                .iter()
                .find(|(key, _)| key == &id)
                .map(|(_, row)| *row)
        }) {
            self.body.scroll_to(index, gtk::ListScrollFlags::NONE, None);
        }
    }
    fn visible_anchor(&self) -> Option<String> {
        self.cards
            .borrow()
            .iter()
            .filter_map(|(id, card)| {
                let bounds = card.compute_bounds(&self.viewport)?;
                (card.is_mapped()
                    && bounds.y() < self.viewport.height() as f32
                    && bounds.y() + bounds.height() > 0.)
                    .then(|| (id.clone(), bounds.y()))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }
    fn select_index(&self, index: usize) {
        let Some((id, row)) = self.order.borrow().get(index).cloned() else {
            return;
        };
        *self.selected.borrow_mut() = id.clone();
        for (key, card) in self.cards.borrow().iter() {
            if key == &id {
                card.add_css_class("task-selected");
            } else {
                card.remove_css_class("task-selected");
            }
        }
        self.body.scroll_to(row, gtk::ListScrollFlags::FOCUS, None);
    }
    fn column_count(&self) -> usize {
        if self.grid.is_active() {
            ((self.viewport.width() + 10) / 268).clamp(1, 3) as usize
        } else {
            1
        }
    }
    fn row(self: &Rc<Self>, row: &Row) -> gtk::Widget {
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        content.set_margin_bottom(10);
        match row {
            Row::Empty => content.append(&gtk::Label::new(Some(
                "No pages here yet\nSave a page or adjust your filters.",
            ))),
            Row::Heading(group) => {
                let label = gtk::Label::new(Some(if group == "All" { "Inbox" } else { group }));
                label.set_xalign(0.);
                label.add_css_class("muted");
                content.append(&label);
            }
            Row::Cards(tasks, columns) => {
                content.set_homogeneous(true);
                for task in tasks {
                    content.append(&self.card(task));
                }
                for _ in tasks.len()..*columns {
                    content.append(&gtk::Box::new(gtk::Orientation::Horizontal, 0));
                }
            }
        }
        content.upcast()
    }
    fn card(self: &Rc<Self>, task: &Value) -> gtk::Widget {
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
            .unwrap_or_default();
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
        if preview.image.as_str().starts_with("file://") {
            let picture = gtk::Picture::new();
            tjik_gtk::images::set_picture(&picture, &preview.image);
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
                if preview.description.as_str().is_empty() {
                    url.split_once(domain).map(|(_, path)| path).unwrap_or("/")
                } else {
                    preview.description.as_str()
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
            if class == "muted" {
                self.preview_widgets
                    .borrow_mut()
                    .entry(url.to_owned())
                    .or_default()
                    .push((art.downgrade(), l.downgrade()));
            }
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
        card.upcast()
    }
}

impl Drop for Reading {
    fn drop(&mut self) {
        if let Some(request) = self.request.get_mut().take() {
            request.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::previews::PreviewFuture;
    use std::{
        cell::Cell,
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };

    #[derive(Default)]
    struct Pending {
        requests: RefCell<Vec<Vec<String>>>,
        response: RefCell<Option<Vec<PagePreview>>>,
        wake: RefCell<Option<Waker>>,
        dropped: Cell<usize>,
    }
    struct Response(Rc<Pending>);
    impl Future for Response {
        type Output = Vec<PagePreview>;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if let Some(response) = self.0.response.borrow_mut().take() {
                Poll::Ready(response)
            } else {
                *self.0.wake.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
    impl Drop for Response {
        fn drop(&mut self) {
            self.0.dropped.set(self.0.dropped.get() + 1);
        }
    }
    struct Provider(Rc<Pending>);
    impl PreviewProvider for Provider {
        fn load(&self, urls: Vec<String>) -> PreviewFuture {
            self.0.requests.borrow_mut().push(urls);
            Box::pin(Response(self.0.clone()))
        }
    }
    fn drain() {
        let context = glib::MainContext::default();
        for _ in 0..100 {
            if !context.pending() {
                break;
            }
            context.iteration(false);
        }
    }
    #[test]
    #[ignore = "requires an isolated GTK display; scripts/test-previews.py"]
    fn previews_bound_batches_reject_stale_results_and_cancel_on_drop() {
        gtk::init().unwrap();
        let picture = gtk::Picture::new();
        let source = format!("data:image/svg+xml;base64,{}", glib::base64_encode(b"<svg xmlns='http://www.w3.org/2000/svg' width='16' height='8'><rect width='16' height='8' fill='red'/></svg>"));
        tjik_gtk::images::set_picture(&picture, &source);
        for _ in 0..200 {
            drain();
            if picture.paintable().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(picture.paintable().unwrap().intrinsic_width(), 16);
        tjik_gtk::images::set_picture(&picture, &source);
        tjik_gtk::images::set_picture(&picture, "https://example.test/unsupported");
        drain();
        assert!(
            picture.paintable().is_none(),
            "stale image populated a cleared picture"
        );
        let pending = Rc::new(Pending::default());
        let view = Reading::new(Some(Rc::new(Provider(pending.clone()))));
        let window = gtk::Window::builder()
            .title("Tasktjik reading fixture")
            .default_width(960)
            .default_height(720)
            .child(&view.root)
            .build();
        window.present();
        let tasks: Vec<Value> = (0..9).map(|i| json!({"id":i.to_string(),"kind":"reading","reference":format!("https://example.test/{i}")})).collect();
        view.update(&json!({"tasks":tasks}));
        view.update(&json!({"tasks":tasks}));
        drain();
        assert_eq!(pending.requests.borrow().len(), 1);
        assert_eq!(pending.requests.borrow()[0].len(), 4);
        let stale = pending.requests.borrow()[0][0].clone();
        view.update(&json!({"tasks":[{"id":"new","kind":"reading","reference":"https://example.test/new"}]}));
        *pending.response.borrow_mut() = Some(vec![PagePreview {
            url: stale.clone(),
            description: "stale".into(),
            ..Default::default()
        }]);
        pending.wake.borrow_mut().take().unwrap().wake();
        drain();
        assert!(!view.previews.borrow().contains_key(&stale));
        assert_eq!(pending.requests.borrow().len(), 2);
        assert_eq!(
            pending.requests.borrow()[1],
            vec!["https://example.test/new"]
        );
        assert_eq!(pending.dropped.get(), 1);
        wait_for(|| !view.cards.borrow().is_empty());
        let card = view.cards.borrow()[0].1.clone();
        *pending.response.borrow_mut() = Some(vec![PagePreview {
            url: "https://example.test/new".into(),
            description: "updated metadata".into(),
            ..Default::default()
        }]);
        pending.wake.borrow_mut().take().unwrap().wake();
        drain();
        assert_eq!(view.cards.borrow()[0].1, card, "metadata rebuilt the card");
        assert_eq!(
            view.preview_widgets.borrow()["https://example.test/new"][0]
                .1
                .upgrade()
                .unwrap()
                .text(),
            "updated metadata"
        );
        view.update(&json!({"tasks":[{"id":"last","kind":"reading","reference":"https://example.test/last"}]}));
        drain();
        drop(view);
        drain();
        assert_eq!(pending.dropped.get(), 3);
        window.close();
    }
    #[track_caller]
    fn wait_for(mut ready: impl FnMut() -> bool) {
        let reached = glib::MainContext::default().block_on(async {
            for _ in 0..1000 {
                if ready() {
                    return true;
                }
                glib::timeout_future(std::time::Duration::from_millis(2)).await;
            }
            false
        });
        assert!(reached, "virtual reading view did not settle");
    }
    #[test]
    #[ignore = "requires an isolated GTK display; scripts/test-previews.py"]
    fn reading_virtualization_preserves_navigation_groups_and_resize() {
        gtk::init().unwrap();
        let view = Reading::new(None);
        let window = gtk::Window::builder()
            .title("Tasktjik reading fixture")
            .default_width(960)
            .default_height(720)
            .child(&view.root)
            .build();
        let tasks: Vec<Value> = (0..2000).map(|i| json!({"id":format!("item-{i}"),"kind":"reading","title":format!("Page {i:04}"),"reference":format!("https://example.test/{i}"),"group":if i < 1000 {"All"} else {"Second"}})).collect();
        view.update(&json!({"tasks":tasks}));
        window.present();
        wait_for(|| view.root.is_mapped() && !view.cards.borrow().is_empty());
        assert_eq!(view.order.borrow().len(), 2000);
        assert!(view.cards.borrow().len() < 500);
        view.select_index(1999);
        let selected = view.selected.borrow().clone();
        wait_for(|| {
            view.cards
                .borrow()
                .iter()
                .any(|(id, card)| id == &selected && card.is_mapped())
        });
        assert!(view
            .cards
            .borrow()
            .iter()
            .find(|(id, _)| id == &selected)
            .unwrap()
            .1
            .has_css_class("task-selected"));
        let opened = Rc::new(RefCell::new(String::new()));
        let result = opened.clone();
        view.connect(move |action, value| {
            assert_eq!(action, "open-reference");
            *result.borrow_mut() = s(&value, "id").into();
        });
        view.emit_selected("open-reference");
        assert_eq!(*opened.borrow(), selected);
        view.categories.set_active(true);
        view.grid.set_active(true);
        wait_for(|| view.columns.get() == 3);
        let headings = (0..view.model.n_items())
            .filter(|i| {
                matches!(
                    *view
                        .model
                        .item(*i)
                        .unwrap()
                        .downcast::<glib::BoxedAnyObject>()
                        .unwrap()
                        .borrow::<Row>(),
                    Row::Heading(_)
                )
            })
            .count();
        assert_eq!(headings, 2);
        assert_eq!(view.order.borrow().len(), 2000);
        // Exercise a real compositor resize; GTK's default size does not shrink
        // an already mapped toplevel on this backend.
        window.set_resizable(true);
        let instance = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").unwrap();
        let output = std::process::Command::new("hyprctl")
            .args(["--instance", &instance, "-j", "clients"])
            .output()
            .unwrap();
        let clients: Value = serde_json::from_slice(&output.stdout).unwrap();
        let address = clients
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["pid"].as_u64() == Some(std::process::id() as u64))
            .unwrap()["address"]
            .as_str()
            .unwrap();
        let command = format!("hl.dispatch(hl.dsp.window.resize({{x=520,y=720,relative=false,window=\"address:{address}\"}}))");
        let output = std::process::Command::new("hyprctl")
            .args(["--instance", &instance, "eval", &command])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout)
            .to_lowercase()
            .contains("error"));
        wait_for(|| view.columns.get() == 1);
        assert!(view.cards.borrow().len() < 500);
        view.filter.set_text("Page 0007");
        view.render();
        assert_eq!(view.order.borrow().len(), 1);
        assert_eq!(view.order.borrow()[0].0, "item-7");
        assert_eq!(view.state.borrow()["tasks"].as_array().unwrap().len(), 2000);
        window.close();
    }
}
