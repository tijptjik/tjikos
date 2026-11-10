//! Reusable Tasktjik view. Mutations use the authority's revisions and request IDs;
//! this crate owns presentation, never recurrence, workflow or persistence rules.
mod dialogs;
mod editor;
#[cfg(test)]
mod performance;
pub mod previews;
mod reading;
pub mod style;
mod viewport;
mod virtual_column;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};
use tjik_gtk::gtk::{self, gdk, glib, prelude::*};
pub(crate) fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
pub(crate) fn button(label: &str, action: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.connect_clicked(move |_| action());
    b
}
pub(crate) fn clear(w: &gtk::Box) {
    while let Some(c) = w.first_child() {
        w.remove(&c);
    }
}
pub struct TaskView {
    pub root: gtk::Box,
    pub stack: gtk::Stack,
    body: gtk::Box,
    scroll: gtk::ScrolledWindow,
    toolbar: gtk::Box,
    options: gtk::FlowBox,
    reading: Rc<reading::Reading>,
    message: gtk::Label,
    filter: gtk::Entry,
    sort: gtk::DropDown,
    dates: gtk::DropDown,
    project: RefCell<String>,
    completed: gtk::CheckButton,
    cancelled: gtk::CheckButton,
    state: RefCell<Value>,
    collection: RefCell<String>,
    selector: gtk::DropDown,
    selected: RefCell<String>,
    send: Rc<dyn Fn(Value)>,
    pub(crate) pending: Cell<bool>,
    pending_action: RefCell<String>,
    dragging: Cell<bool>,
    rendered: RefCell<Value>,
    navigation: RefCell<BTreeMap<String, glib::WeakRef<gtk::Box>>>,
    order: RefCell<Vec<(String, glib::WeakRef<virtual_column::VirtualColumn>, usize)>>,
    pub(crate) editor: RefCell<Option<editor::Editor>>,
}
impl TaskView {
    pub fn new(
        send: impl Fn(Value) + 'static,
        exit: impl Fn() + 'static,
        previews: Option<Rc<dyn previews::PreviewProvider>>,
    ) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        root.set_focusable(true);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let exit = Rc::new(exit);
        let leave = exit.clone();
        bar.append(&button("‹ Launcher", move || leave()));
        let filter = gtk::Entry::builder()
            .placeholder_text("Filter tasks, groups or tags…")
            .hexpand(true)
            .build();
        bar.append(&filter);
        let add = gtk::Button::with_label("New");
        bar.append(&add);
        root.append(&bar);
        let options = gtk::FlowBox::new();
        options.set_selection_mode(gtk::SelectionMode::None);
        options.set_min_children_per_line(1);
        options.set_max_children_per_line(7);
        let collection =
            gtk::DropDown::from_strings(&["Tasks", "Reading", "Projects", "Templates"]);
        options.insert(&collection, -1);
        let sort = gtk::DropDown::from_strings(&[
            "Due ↑",
            "Due ↓",
            "Title ↑",
            "Title ↓",
            "Manual",
            "Progress",
        ]);
        options.insert(&sort, -1);
        let dates = gtk::DropDown::from_strings(&["All dates", "Today", "This week", "This month"]);
        options.insert(&dates, -1);
        let completed = gtk::CheckButton::with_label("Completed");
        let cancelled = gtk::CheckButton::with_label("Cancelled");
        options.insert(&completed, -1);
        options.insert(&cancelled, -1);
        let undo = gtk::Button::with_label("Undo");
        options.insert(&undo, -1);
        root.append(&options);
        let message = gtk::Label::new(None);
        message.set_wrap(true);
        message.add_css_class("error");
        root.append(&message);
        let stack = gtk::Stack::new();
        stack.set_hhomogeneous(false);
        stack.set_vhomogeneous(false);
        stack.set_vexpand(true);
        root.append(&stack);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&body)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        stack.add_named(&scroll, Some("tasks"));
        let reading = reading::Reading::new(previews);
        stack.add_named(&reading.root, Some("reading"));
        let view = Rc::new(Self {
            root,
            stack,
            body,
            scroll,
            toolbar: bar,
            options: options.clone(),
            reading,
            message,
            filter,
            sort,
            dates,
            project: RefCell::new(String::new()),
            completed,
            cancelled,
            state: RefCell::new(json!({})),
            collection: RefCell::new("todo".into()),
            selector: collection.clone(),
            selected: RefCell::new(String::new()),
            send: Rc::new(send),
            pending: Cell::new(false),
            pending_action: RefCell::new(String::new()),
            dragging: Cell::new(false),
            rendered: RefCell::new(Value::Null),
            navigation: RefCell::new(BTreeMap::new()),
            order: RefCell::new(Vec::new()),
            editor: RefCell::new(None),
        });
        let weak = Rc::downgrade(&view);
        view.reading.connect(move |action, task| {
            if let Some(v) = weak.upgrade() {
                match action {
                    "exit" => exit(),
                    "collection" => {
                        v.selector.set_selected(0);
                    }
                    "window" => {
                        (v.send)(json!({"op":"gtk-open-task-window","collection":"reading"}));
                    }
                    "new" => {
                        v.edit(json!({"kind":"reading","title":"","group":"All","subtasks":[]}))
                    }
                    "edit" => {
                        if !s(&task, "id").is_empty() {
                            v.edit(task);
                        }
                    }
                    "undo" => v.request(json!({"action":"undo"})),
                    action => {
                        if !s(&task, "id").is_empty() {
                            v.action(action, &task, json!({}));
                        }
                    }
                }
            }
        });
        let weak = Rc::downgrade(&view);
        options.insert(
            &button("Groups", move || {
                if let Some(v) = weak.upgrade() {
                    v.groups();
                }
            }),
            -1,
        );
        let weak = Rc::downgrade(&view);
        view.filter.connect_changed(move |_| {
            if let Some(v) = weak.upgrade() {
                v.render();
            }
        });
        let weak = Rc::downgrade(&view);
        view.dates.connect_selected_notify(move |_| {
            if let Some(v) = weak.upgrade() {
                v.render();
            }
        });
        let weak = Rc::downgrade(&view);
        view.sort.connect_selected_notify(move |_| {
            if let Some(v) = weak.upgrade() {
                v.render();
            }
        });
        for toggle in [&view.completed, &view.cancelled] {
            let weak = Rc::downgrade(&view);
            toggle.connect_toggled(move |_| {
                if let Some(v) = weak.upgrade() {
                    v.render();
                }
            });
        }
        let weak = Rc::downgrade(&view);
        collection.connect_selected_notify(move |c| {
            if let Some(v) = weak.upgrade() {
                v.project.borrow_mut().clear();
                *v.collection.borrow_mut() =
                    ["todo", "reading", "project", "template"][c.selected() as usize].into();
                v.render();
            }
        });
        let weak = Rc::downgrade(&view);
        add.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                let kind = v.collection.borrow().clone();
                v.edit(json!({"kind":kind,"group":"All","title":"","subtasks":[],"project":v.project.borrow().clone()}));
            }
        });
        let weak = Rc::downgrade(&view);
        undo.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                v.request(json!({"action":"undo"}));
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&view);
        keys.connect_key_pressed(move |_, key, _, mods| {
            let Some(v) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if v.stack.visible_child_name().as_deref() != Some("tasks") {
                return glib::Propagation::Proceed;
            }
            // Editable children own ordinary text and navigation keys.
            if key == gdk::Key::Escape && v.filter.has_focus() {
                v.filter.set_text("");
                v.root.grab_focus();
                return glib::Propagation::Stop;
            }
            if v.filter.has_focus() && !mods.contains(gdk::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }
            match key {
                gdk::Key::slash => {
                    v.filter.grab_focus();
                }
                gdk::Key::Down | gdk::Key::Up => {
                    let items = v.order.borrow();
                    if !items.is_empty() {
                        let current = items
                            .iter()
                            .position(|(id, _, _)| *id == *v.selected.borrow());
                        let delta = if key == gdk::Key::Down { 1 } else { -1 };
                        let index = current
                            .map(|i| (i as i32 + delta).rem_euclid(items.len() as i32) as usize)
                            .unwrap_or(0);
                        let id = items[index].0.clone();
                        drop(items);
                        v.focus_task(&id);
                    }
                }
                gdk::Key::Escape => {
                    v.filter.set_text("");
                    v.filter.grab_focus();
                }

                gdk::Key::n | gdk::Key::N => {
                    v.edit(json!({"kind":v.collection.borrow().clone(),"title":"","group":"All"}));
                }
                gdk::Key::z if mods.contains(gdk::ModifierType::CONTROL_MASK) => {
                    v.request(json!({"action":"undo"}))
                }
                gdk::Key::Return => {
                    if let Some(t) = v.selected_task() {
                        v.edit(t);
                    }
                }
                gdk::Key::space => {
                    if let Some(t) = v.selected_task() {
                        v.action("toggle", &t, json!({}));
                    }
                }
                _ => return glib::Propagation::Proceed,
            };
            glib::Propagation::Stop
        });
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        view.root.add_controller(keys);
        view
    }
    pub fn update(self: &Rc<Self>, value: &Value) {
        if let Some(source) = value["source"].as_str() {
            self.pending.set(false);
            self.show_image(source);
            return;
        }
        if let Some(map) = value.as_object() {
            let mut state = self.state.borrow_mut();
            if !state.is_object() {
                *state = json!({});
            }
            for (k, v) in map {
                state[k] = v.clone();
            }
        }
        // Only our acknowledged attachment may advance an open editor's revision.
        // A concurrent remote edit must still cause a conflict on Save.
        if value["mutation_accepted"] == true && *self.pending_action.borrow() == "attach-image" {
            if let Some(editor) = self.editor.borrow().as_ref() {
                let id = editor.task.borrow()["id"].clone();
                if let Some(task) = value["tasks"]
                    .as_array()
                    .and_then(|ts| ts.iter().find(|t| t["id"] == id))
                {
                    *editor.task.borrow_mut() = task.clone();
                }
            }
        }
        if value["mutation_accepted"] == true
            || ["refresh", "list", "plans", "open-reference"]
                .contains(&self.pending_action.borrow().as_str())
        {
            self.pending.set(false);
            if self
                .editor
                .borrow()
                .as_ref()
                .is_some_and(|e| e.saving.get())
            {
                self.close_editor();
            }
        }
        if let Some(collection) = value["collection"].as_str() {
            self.selector.set_selected(
                ["todo", "reading", "project", "template"]
                    .iter()
                    .position(|c| *c == collection)
                    .unwrap_or(0) as u32,
            );
            *self.collection.borrow_mut() = collection.into();
        }
        self.message.set_text(s(value, "sync_message"));
        self.message
            .set_visible(!s(value, "sync_message").is_empty());
        self.render();
        if !s(value, "draft_id").is_empty() && self.editor.borrow().is_none() {
            self.edit(value["draft"].clone());
        }
    }
    pub fn error(&self, message: &str) {
        self.pending.set(false);
        self.message.set_text(message);
        self.message.set_visible(true);
        if let Some(e) = self.editor.borrow().as_ref() {
            e.saving.set(false);
            e.root.set_sensitive(true);
            e.form.set_sensitive(true);
        }
    }
    pub fn state(&self) -> Value {
        let mut value = self.state.borrow().clone();
        value["gtk_view"] = json!(self.stack.visible_child_name().as_deref());
        value
    }
    pub fn is_editing(&self) -> bool {
        self.stack.visible_child_name().as_deref() == Some("editor")
    }
    pub fn focus(&self) {
        if self.collection.borrow().as_str() == "reading" {
            self.reading.root.grab_focus();
            return;
        }
        self.filter.grab_focus();
    }
    pub(crate) fn request(&self, mut request: Value) {
        if self.pending.replace(true) {
            return;
        }
        self.message.set_text("");
        self.message.set_visible(false);
        request["op"] = json!("app-action");
        request["provider"] = json!("Todo");
        if request["request_id"].is_null() {
            request["request_id"] = json!(uuid::Uuid::new_v4().to_string());
        }
        *self.pending_action.borrow_mut() = s(&request, "action").to_owned();
        (self.send)(request);
    }
    fn selected_task(&self) -> Option<Value> {
        self.state.borrow()["tasks"]
            .as_array()?
            .iter()
            .find(|t| s(t, "id") == self.selected.borrow().as_str())
            .cloned()
    }
    fn action(&self, action: &str, task: &Value, extra: Value) {
        let mut r = json!({"action":action,"id":task["id"],"revision":task["revision"],"period":task["period"],"actor":"user"});
        if let Some(map) = extra.as_object() {
            for (k, v) in map {
                r[k] = v.clone();
            }
        }
        self.request(r);
    }
    pub fn refresh(&self) {
        if !self.pending.get() && !self.is_editing() {
            self.request(json!({"action":"refresh"}));
        }
    }
    pub fn render(self: &Rc<Self>) {
        // Keep an in-progress editor and its input focus intact during sync updates.
        if self.is_editing() {
            return;
        }
        if self.pending.get() || self.dragging.get() {
            return;
        }
        let signature = json!([
            self.state.borrow()["tasks"],
            self.collection.borrow().as_str(),
            self.filter.text().as_str(),
            self.sort.selected(),
            self.dates.selected(),
            self.project.borrow().as_str(),
            self.completed.is_active(),
            self.cancelled.is_active()
        ]);
        if *self.rendered.borrow() == signature {
            return;
        }
        *self.rendered.borrow_mut() = signature;
        let reading = self.collection.borrow().as_str() == "reading";
        self.toolbar.set_visible(!reading);
        self.options.set_visible(!reading);
        if reading {
            self.reading.update(&self.state.borrow());
            self.stack.set_visible_child_name("reading");
            return;
        }
        self.stack.set_visible_child_name("tasks");
        let restore_focus = self
            .root
            .root()
            .and_then(|r| r.downcast::<gtk::Window>().ok())
            .and_then(|w| gtk::prelude::GtkWindowExt::focus(&w))
            .is_some_and(|focus| {
                self.navigation
                    .borrow()
                    .iter()
                    .filter_map(|(_, card)| card.upgrade())
                    .any(|card| focus == card || focus.is_ancestor(&card))
            });
        self.navigation.borrow_mut().clear();
        self.order.borrow_mut().clear();
        clear(&self.body);
        let state = self.state.borrow().clone();
        let collection = self.collection.borrow().clone();
        let filter = self.filter.text().to_lowercase();
        let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let horizon = chrono::Local::now().date_naive()
            + chrono::Duration::days(match self.dates.selected() {
                1 => 0,
                2 => 7,
                _ => 30,
            });
        let horizon = horizon.to_string();
        for task in state["tasks"].as_array().into_iter().flatten() {
            let kind = s(task, "kind");
            if kind != collection && !(collection == "todo" && kind == "project") {
                continue;
            }
            if !self.project.borrow().is_empty() {
                if s(task, "project") != self.project.borrow().as_str() {
                    continue;
                }
            } else if task["onlyInProject"] == true && !s(task, "project").is_empty() {
                continue;
            }
            let due = if !s(task, "effectiveDue").is_empty() {
                s(task, "effectiveDue")
            } else {
                s(task, "due")
            };
            if self.dates.selected() > 0 && !due.is_empty() && due > horizon.as_str() {
                continue;
            }
            if task["done"] == true && !self.completed.is_active() {
                continue;
            }
            if s(task, "status") == "cancelled" && !self.cancelled.is_active() {
                continue;
            }
            if !filter.is_empty()
                && !format!("{} {} {}", s(task, "title"), s(task, "group"), task["tags"])
                    .to_lowercase()
                    .contains(&filter)
            {
                continue;
            }
            groups
                .entry(s(task, "group").into())
                .or_default()
                .push(task.clone());
        }
        if groups.is_empty() {
            let label = gtk::Label::new(Some("No matching items"));
            label.add_css_class("muted");
            self.body.append(&label);
        }
        if !self.project.borrow().is_empty() {
            let weak = Rc::downgrade(self);
            self.body.append(&button("‹ All projects", move || {
                if let Some(v) = weak.upgrade() {
                    v.project.borrow_mut().clear();
                    v.render();
                }
            }));
        }
        let columns = gtk::FlowBox::new();
        columns.set_selection_mode(gtk::SelectionMode::None);
        columns.set_homogeneous(true);
        columns.set_max_children_per_line(4);
        columns.set_min_children_per_line(1);
        columns.set_column_spacing(12);
        columns.set_row_spacing(12);
        self.body.append(&columns);
        for (group, mut tasks) in groups {
            let sort = self.sort.selected();
            tasks.sort_by(|a, b| {
                let key = match sort {
                    0 | 1 => "due",
                    4 => "manualOrder",
                    5 => "status",
                    _ => "title",
                };
                if sort == 5 {
                    let ratio = |t: &Value| {
                        t["subtaskProgress"]["done"].as_f64().unwrap_or(0.)
                            / t["subtaskProgress"]["total"].as_f64().unwrap_or(1.).max(1.)
                    };
                    return ratio(b)
                        .total_cmp(&ratio(a))
                        .then(s(a, "title").cmp(s(b, "title")));
                }
                if (sort == 0 || sort == 1) && s(a, key).is_empty() != s(b, key).is_empty() {
                    return s(a, key).is_empty().cmp(&s(b, key).is_empty());
                }
                let order = s(a, key)
                    .cmp(s(b, key))
                    .then(s(a, "title").cmp(s(b, "title")));
                if sort == 1 || sort == 3 {
                    order.reverse()
                } else {
                    order
                }
            });
            let heading = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let label = gtk::Label::builder()
                .label(&group)
                .xalign(0.0)
                .hexpand(true)
                .build();
            label.add_css_class("accent");
            heading.append(&label);
            let weak = Rc::downgrade(self);
            let name = group.clone();
            heading.append(&button("Rename", move || {
                if let Some(v) = weak.upgrade() {
                    v.rename_group(&name);
                }
            }));
            let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
            panel.set_size_request(280, -1);
            panel.set_valign(gtk::Align::Start);
            panel.append(&heading);
            let tasks = Rc::new(tasks);
            let weak = Rc::downgrade(self);
            let records = tasks.clone();
            let list =
                virtual_column::VirtualColumn::new(&self.scroll, tasks.len(), move |index| {
                    weak.upgrade()
                        .map(|v| v.card(records[index].clone()).upcast())
                        .unwrap_or_else(|| gtk::Box::new(gtk::Orientation::Vertical, 0).upcast())
                });
            self.order.borrow_mut().extend(
                tasks
                    .iter()
                    .enumerate()
                    .map(|(index, task)| (s(task, "id").to_owned(), list.downgrade(), index)),
            );
            let weak = Rc::downgrade(self);
            let name = group.clone();
            let drop = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
            drop.connect_drop(move |_, value, _, _| {
                let Ok(id) = value.get::<String>() else {
                    return false;
                };
                let Some(v) = weak.upgrade() else {
                    return false;
                };
                let task = v.state.borrow()["tasks"]
                    .as_array()
                    .and_then(|ts| ts.iter().find(|t| s(t, "id") == id))
                    .cloned();
                if let Some(t) = task {
                    v.action("move", &t, json!({"group":name,"before_id":""}));
                    true
                } else {
                    false
                }
            });
            list.add_controller(drop);
            panel.append(&list);
            columns.insert(&panel, -1);
        }
        if restore_focus {
            let order = self.order.borrow();
            let id = order
                .iter()
                .find(|(id, _, _)| *id == *self.selected.borrow())
                .or_else(|| order.first())
                .map(|entry| entry.0.clone());
            drop(order);
            if let Some(id) = id {
                self.focus_task(&id);
            }
        }
    }
    fn focus_task(&self, id: &str) {
        let target = self
            .order
            .borrow()
            .iter()
            .find(|(key, _, _)| key == id)
            .and_then(|(_, column, index)| column.upgrade().map(|column| (column, *index)));
        if let Some((column, index)) = target {
            *self.selected.borrow_mut() = id.to_owned();
            column.reveal(index);
        }
    }
    fn pin_task(&self, id: &str, pinned: bool) {
        if let Some((_, column, index)) = self.order.borrow().iter().find(|(key, _, _)| key == id) {
            if let Some(column) = column.upgrade() {
                column.pin(pinned.then_some(*index));
            }
        }
    }
    fn card(self: &Rc<Self>, task: Value) -> gtk::Box {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
        card.add_css_class("card");
        card.set_focusable(true);
        if s(&task, "id") == self.selected.borrow().as_str() {
            card.add_css_class("task-selected");
        }
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let done = gtk::CheckButton::new();
        done.set_active(task["done"] == true);
        done.set_sensitive(!["project", "template"].contains(&s(&task, "kind")));
        row.append(&done);
        let title = gtk::Label::builder()
            .label(s(&task, "title"))
            .xalign(0.0)
            .wrap(true)
            .hexpand(true)
            .build();
        row.append(&title);
        if s(&task, "kind") == "project" {
            let weak = Rc::downgrade(self);
            let id = s(&task, "id").to_owned();
            row.append(&button("Tasks", move || {
                if let Some(v) = weak.upgrade() {
                    v.selector.set_selected(0);
                    *v.project.borrow_mut() = id.clone();
                    *v.collection.borrow_mut() = "todo".into();
                    v.render();
                }
            }));
        }
        let weak = Rc::downgrade(self);
        let t = task.clone();
        done.connect_toggled(move |_| {
            if let Some(v) = weak.upgrade() {
                v.action("toggle", &t, json!({}));
            }
        });
        let weak = Rc::downgrade(self);
        let t = task.clone();
        row.append(&button("Edit", move || {
            if let Some(v) = weak.upgrade() {
                v.edit(t.clone());
            }
        }));
        card.append(&row);
        let details = format!(
            "{}{}{}",
            s(&task, "status"),
            if s(&task, "due").is_empty() {
                String::new()
            } else {
                format!(" · {}", s(&task, "due"))
            },
            if s(&task, "reference").is_empty() {
                String::new()
            } else {
                format!(" · {}", s(&task, "reference"))
            }
        );
        let label = gtk::Label::builder()
            .label(details)
            .xalign(0.0)
            .wrap(true)
            .build();
        label.add_css_class("muted");
        card.append(&label);
        self.subtasks(&card, &task);
        let actions = gtk::FlowBox::new();
        actions.set_selection_mode(gtk::SelectionMode::None);
        actions.set_max_children_per_line(3);
        actions.set_min_children_per_line(1);
        for (label, action, extra) in [
            ("Open", "open-reference", json!({})),
            (
                "Snooze",
                "snooze",
                json!({"until":(chrono::Utc::now()+chrono::Duration::days(1)).to_rfc3339()}),
            ),
            (
                "Cancel",
                "status",
                json!({"status":if s(&task,"status")=="cancelled"{"ready"}else{"cancelled"}}),
            ),
            ("Verify", "verify", json!({})),
            ("Delete", "purge", json!({})),
        ] {
            if ["project", "template"].contains(&s(&task, "kind"))
                && ["status", "verify", "snooze"].contains(&action)
            {
                continue;
            }
            if action == "open-reference" && s(&task, "reference").is_empty() {
                continue;
            }
            let weak = Rc::downgrade(self);
            let t = task.clone();
            actions.insert(
                &button(label, move || {
                    if let Some(v) = weak.upgrade() {
                        if action == "snooze" {
                            v.snooze(t.clone());
                        } else if action == "verify" {
                            v.verify(t.clone());
                        } else if action == "purge" {
                            v.confirm_delete(t.clone());
                        } else {
                            v.action(action, &t, extra.clone());
                        }
                    }
                }),
                -1,
            );
        }
        for (index, image) in task["images"].as_array().into_iter().flatten().enumerate() {
            let weak = Rc::downgrade(self);
            let id = task["id"].clone();
            let image_id = image["id"].clone();
            actions.insert(
                &button(&format!("Image {}", index + 1), move || {
                    if let Some(v) = weak.upgrade() {
                        v.request(json!({"action":"image-get","id":id,"image_id":image_id}));
                    }
                }),
                -1,
            );
        }
        let weak = Rc::downgrade(self);
        let t = task.clone();
        actions.insert(
            &button("Paste image", move || {
                if let Some(v) = weak.upgrade() {
                    v.paste_image(t.clone());
                }
            }),
            -1,
        );
        card.append(&actions);
        let source = gtk::DragSource::new();
        source.set_actions(gdk::DragAction::MOVE);
        let id = s(&task, "id").to_owned();
        source
            .connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&id.to_value())));
        let weak = Rc::downgrade(self);
        let drag_id = s(&task, "id").to_owned();
        source.connect_drag_begin(move |_, _| {
            if let Some(v) = weak.upgrade() {
                v.dragging.set(true);
                v.pin_task(&drag_id, true);
            }
        });
        let weak = Rc::downgrade(self);
        let drag_id = s(&task, "id").to_owned();
        source.connect_drag_end(move |_, _, _| {
            if let Some(v) = weak.upgrade() {
                v.dragging.set(false);
                v.pin_task(&drag_id, false);
                v.render();
            }
        });
        card.add_controller(source);
        let weak = Rc::downgrade(self);
        let target = task.clone();
        let drop = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
        drop.connect_drop(move |_, value, _, _| {
            let Ok(id) = value.get::<String>() else {
                return false;
            };
            let Some(v) = weak.upgrade() else {
                return false;
            };
            let source = v.state.borrow()["tasks"]
                .as_array()
                .and_then(|ts| ts.iter().find(|t| s(t, "id") == id))
                .cloned();
            if let Some(t) = source {
                v.action(
                    "move",
                    &t,
                    json!({"group":target["group"],"before_id":target["id"]}),
                );
                true
            } else {
                false
            }
        });
        card.add_controller(drop);
        let click = gtk::GestureClick::new();
        let weak = Rc::downgrade(self);
        let id = s(&task, "id").to_owned();
        click.connect_pressed(move |gesture, _, _, _| {
            if let Some(v) = weak.upgrade() {
                *v.selected.borrow_mut() = id.clone();
                if let Some(widget) = gesture.widget() {
                    widget.grab_focus();
                }
            }
        });
        card.add_controller(click);
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(self);
        let id = s(&task, "id").to_owned();
        focus.connect_enter(move |_| {
            if let Some(v) = weak.upgrade() {
                *v.selected.borrow_mut() = id.clone();
                v.navigation.borrow_mut().retain(|key, weak| {
                    let Some(card) = weak.upgrade() else {
                        return false;
                    };
                    if *key == id {
                        card.add_css_class("task-selected");
                    } else {
                        card.remove_css_class("task-selected");
                    }
                    true
                });
            }
        });
        card.add_controller(focus);
        self.navigation
            .borrow_mut()
            .insert(s(&task, "id").to_owned(), card.downgrade());
        card
    }
    fn subtasks(self: &Rc<Self>, parent: &gtk::Box, task: &Value) {
        for node in task["workflowRows"].as_array().into_iter().flatten() {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            row.set_margin_start(node["depth"].as_i64().unwrap_or(0) as i32 * 16);
            let b = gtk::CheckButton::with_label(s(node, "title"));
            b.set_hexpand(true);
            b.set_active(s(node, "status") == "done");
            b.set_sensitive(
                !["blocked", "cancelled"].contains(&s(node, "status"))
                    && node["blocked"] != true
                    && !["flow", "group"].contains(&s(node, "kind")),
            );
            b.set_tooltip_text(Some(s(node, "status")));
            let weak = Rc::downgrade(self);
            let t = task.clone();
            let id = node["id"].clone();
            b.connect_toggled(move |_| {
                if let Some(v) = weak.upgrade() {
                    v.action("subtask-toggle", &t, json!({"subtask_id":id}));
                }
            });
            row.append(&b);
            if node["blockId"].is_string() {
                let weak = Rc::downgrade(self);
                let t = task.clone();
                let id = node["id"].clone();
                row.append(&button(
                    if s(node, "status") == "cancelled" {
                        "Reopen"
                    } else {
                        "Cancel"
                    },
                    move || {
                        if let Some(v) = weak.upgrade() {
                            v.action("subtask-cancel", &t, json!({"subtask_id":id}));
                        }
                    },
                ));
            }
            parent.append(&row);
        }
    }
    pub(crate) fn close_editor(&self) {
        self.stack
            .set_visible_child_name(if self.collection.borrow().as_str() == "reading" {
                "reading"
            } else {
                "tasks"
            });
        *self.rendered.borrow_mut() = Value::Null;
        if let Some(editor) = self.editor.borrow_mut().take() {
            self.stack.remove(&editor.root);
        }
        self.pending.set(false);
        let selected = self.selected.borrow().clone();
        self.focus_task(&selected);
    }

    fn confirm_delete(self: &Rc<Self>, task: Value) {
        let dialog = gtk::Box::new(gtk::Orientation::Vertical, 12);
        dialog.append(&gtk::Label::new(Some(&format!(
            "Delete ‘{}’?",
            s(&task, "title")
        ))));
        let weak = Rc::downgrade(self);
        dialog.append(&button("Delete", move || {
            if let Some(v) = weak.upgrade() {
                v.stack.set_visible_child_name("tasks");
                v.action("purge", &task, json!({}));
            }
        }));
        let weak = Rc::downgrade(self);
        dialog.append(&button("Cancel", move || {
            if let Some(v) = weak.upgrade() {
                v.stack.set_visible_child_name("tasks");
            }
        }));
        if let Some(old) = self.stack.child_by_name("confirm") {
            self.stack.remove(&old);
        }
        self.stack.add_named(&dialog, Some("confirm"));
        self.stack.set_visible_child_name("confirm");
    }
    fn rename_group(self: &Rc<Self>, name: &str) {
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let entry = gtk::Entry::builder().text(name).build();
        panel.append(&entry);
        let weak = Rc::downgrade(self);
        let old = name.to_owned();
        panel.append(&button("Rename group",move ||{if let Some(v)=weak.upgrade(){v.stack.set_visible_child_name("tasks");v.request(json!({"action":"group-rename","source":old,"target":entry.text().as_str(),"groupsRevision":v.state.borrow()["groupsRevision"],"scoped":true,"kind":v.collection.borrow().clone(),"members":v.state.borrow()["tasks"].as_array().into_iter().flatten().filter(|t| s(t,"group")==old && (s(t,"kind")==v.collection.borrow().as_str() || (v.collection.borrow().as_str()=="todo" && s(t,"kind")=="project"))).map(|t|json!({"id":t["id"],"revision":t["revision"]})).collect::<Vec<_>>()}));}}));
        let weak = Rc::downgrade(self);
        panel.append(&button("Cancel", move || {
            if let Some(v) = weak.upgrade() {
                v.stack.set_visible_child_name("tasks");
            }
        }));
        if let Some(old) = self.stack.child_by_name("rename") {
            self.stack.remove(&old);
        }
        self.stack.add_named(&panel, Some("rename"));
        self.stack.set_visible_child_name("rename");
    }
}
