//! Reusable Tasktjik view. Mutations use the authority's revisions and request IDs;
//! this crate owns presentation, never recurrence, workflow or persistence rules.
mod dialogs;
mod editor;
mod icons;
mod input;
mod marker;
mod media;
mod mutations;
#[cfg(test)]
mod performance;
pub mod preferences;
mod presentation;
pub mod preview_helper;
pub mod previews;
mod reading;
mod snooze;
mod status_filter;
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
    if label == "✎" || label == "⚙" {
        let pencil = label == "✎";
        let drawing = gtk::DrawingArea::builder()
            .content_width(16)
            .content_height(16)
            .build();
        drawing.set_draw_func(move |area, ctx, _, _| {
            let color = area.color();
            ctx.set_source_rgba(
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                color.alpha() as f64,
            );
            ctx.set_line_width(1.4);
            if pencil {
                ctx.move_to(3., 10.);
                ctx.line_to(10., 3.);
                ctx.line_to(13., 6.);
                ctx.line_to(6., 13.);
                ctx.line_to(3., 13.);
                ctx.close_path();
                ctx.move_to(8., 5.);
                ctx.line_to(11., 8.);
            } else {
                for (y, x) in [(4., 6.), (8., 10.), (12., 5.)] {
                    ctx.move_to(2., y);
                    ctx.line_to(14., y);
                    ctx.move_to(x, y - 2.);
                    ctx.line_to(x, y + 2.);
                }
            }
            let _ = ctx.stroke();
        });
        b.set_child(Some(&drawing));
        b.set_tooltip_text(Some(if pencil { "Edit" } else { "Tasktjik settings" }));
    } else if label == "⌕" {
        b.set_child(Some(&icons::icon("search")));
        b.set_tooltip_text(Some("Filter tasks (/)"));
    }
    b.connect_clicked(move |_| action());
    b
}
pub(crate) fn clear(w: &gtk::Box) {
    while let Some(c) = w.first_child() {
        w.remove(&c);
    }
}
pub(crate) fn focus_within(widget: &impl IsA<gtk::Widget>) -> bool {
    widget
        .root()
        .and_then(|r| r.downcast::<gtk::Window>().ok())
        .and_then(|w| gtk::prelude::GtkWindowExt::focus(&w))
        .is_some_and(|focus| focus == *widget.as_ref() || focus.is_ancestor(widget))
}
fn visual_tasks(value: &Value) -> Value {
    let mut value = value.clone();
    if let Some(rows) = value.as_array_mut() {
        rows.sort_by(|a, b| s(a, "id").cmp(s(b, "id")));
        for task in rows {
            if let Some(fields) = task.as_object_mut() {
                for key in [
                    "revision",
                    "events",
                    "updatedAt",
                    "createKey",
                    "completedAt",
                ] {
                    fields.remove(key);
                }
                if let Some(completions) =
                    fields.get_mut("completions").and_then(Value::as_object_mut)
                {
                    for value in completions.values_mut() {
                        *value = json!(true);
                    }
                }
            }
        }
    }
    value
}
pub struct TaskView {
    pub root: gtk::Box,
    pub stack: gtk::Stack,
    body: gtk::Box,
    column_count: Cell<i32>,
    scroll: gtk::ScrolledWindow,
    toolbar: gtk::Box,
    heading: RefCell<gtk::Label>,
    count: gtk::Label,
    exit: Rc<dyn Fn()>,
    domain_order: RefCell<Vec<String>>,
    collapsed: RefCell<std::collections::BTreeSet<String>>,
    expanded: RefCell<std::collections::BTreeSet<String>>,
    pending_d: RefCell<Option<(String, glib::SourceId)>>,
    pending_click: RefCell<Option<glib::SourceId>>,
    drag_scroll: Cell<f64>,
    preferences: RefCell<Value>,
    save_preferences: RefCell<Option<Rc<dyn Fn(&Value)>>>,
    options: gtk::FlowBox,
    reading: Rc<reading::Reading>,
    message: gtk::Label,
    filter: gtk::Entry,
    sort: gtk::DropDown,
    dates: gtk::DropDown,
    project: RefCell<String>,
    completed: status_filter::StatusFilter,
    cancelled: status_filter::StatusFilter,
    blocked: status_filter::StatusFilter,
    details: gtk::ToggleButton,
    state: RefCell<Value>,
    collection: RefCell<String>,
    selector: gtk::DropDown,
    selected: RefCell<String>,
    send: Rc<dyn Fn(Value)>,
    pub(crate) pending: Cell<bool>,
    pending_action: RefCell<String>,
    mutations: RefCell<mutations::Mutations>,
    motion_epoch: Cell<u64>,
    outbox: RefCell<Option<std::path::PathBuf>>,
    recover: gtk::Button,
    media_busy: Cell<bool>,
    media_epoch: Cell<u64>,
    dragging: Cell<bool>,
    drag_task: RefCell<Option<Value>>,
    move_projection: RefCell<Option<Value>>,
    empty_targets: gtk::Box,
    rendered: RefCell<Value>,
    completing: Cell<bool>,
    completion_rows: RefCell<Vec<(gtk::Box, Option<i64>, bool)>>,
    markers: RefCell<BTreeMap<String, marker::Marker>>,
    flip_positions: RefCell<BTreeMap<String, (f32, f32)>>,
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
        let root = gtk::Box::new(gtk::Orientation::Vertical, 18);
        root.add_css_class("tasktjik");
        root.set_focusable(true);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let exit = Rc::new(exit);
        let titles = gtk::Box::new(gtk::Orientation::Vertical, 3);
        titles.set_hexpand(true);
        let heading = gtk::Label::builder().label("Tasks").xalign(0.).build();
        heading.add_css_class("task-heading");
        let count = gtk::Label::builder().xalign(0.).build();
        count.add_css_class("muted");
        titles.append(&heading);
        titles.append(&count);
        bar.append(&titles);
        bar.set_valign(gtk::Align::Start);
        let filter = gtk::Entry::builder()
            .placeholder_text("Filter tasks, groups or tags…")
            .hexpand(true)
            .build();
        filter.set_visible(false);
        let add = gtk::Button::with_label("+ New");
        add.add_css_class("primary");
        add.set_valign(gtk::Align::Center);
        bar.append(&add);
        root.append(&bar);
        let options = gtk::FlowBox::new();
        options.set_selection_mode(gtk::SelectionMode::None);
        options.set_min_children_per_line(1);
        options.set_max_children_per_line(2);
        options.set_column_spacing(8);
        options.set_row_spacing(4);
        options.add_css_class("task-toolbar");
        let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        controls.set_halign(gtk::Align::End);
        options.insert(&navigation, -1);
        options.insert(&controls, -1);
        let collection = gtk::DropDown::from_strings(&["Todo", "Reading", "Projects", "Templates"]);
        collection.set_valign(gtk::Align::Center);
        bar.insert_child_after(&collection, Some(&titles));
        let sort = gtk::DropDown::from_strings(&[
            "Due: soonest",
            "Due: latest",
            "Title: A–Z",
            "Title: Z–A",
            "Manual",
            "Progress",
            "Recently completed",
        ]);
        sort.set_widget_name("task-sort");
        controls.append(&sort);
        let dates = gtk::DropDown::from_strings(&["All dates", "Today", "This week", "This month"]);
        dates.set_visible(false);
        controls.append(&dates);
        let completed = status_filter::StatusFilter::new("Completed tasks", "completed");
        let cancelled = status_filter::StatusFilter::new("Cancelled tasks", "cancelled");
        let blocked = status_filter::StatusFilter::new("Blocked tasks", "blocked");
        controls.append(&completed.button);
        controls.append(&cancelled.button);
        controls.append(&blocked.button);
        let details = gtk::ToggleButton::new();
        details.set_child(Some(&icons::icon("details")));
        details.set_tooltip_text(Some("Show task details"));
        controls.append(&details);
        let undo = gtk::Button::with_label("↶");
        undo.set_child(Some(&icons::icon("undo")));
        undo.add_css_class("icon-button");
        undo.set_tooltip_text(Some("Undo (Ctrl+Z)"));

        root.append(&options);
        root.append(&filter);
        let message = gtk::Label::new(None);
        message.set_wrap(true);
        message.add_css_class("error");
        message.set_visible(false);
        root.append(&message);
        let recover = gtk::Button::with_label("Recover unsaved edit");
        recover.set_visible(false);
        recover.set_halign(gtk::Align::Start);
        root.append(&recover);
        let stack = gtk::Stack::new();
        stack.set_hhomogeneous(false);
        stack.set_vhomogeneous(false);
        stack.set_vexpand(true);
        root.append(&stack);
        let empty_targets = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        empty_targets.set_visible(false);
        root.append(&empty_targets);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&body)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let task_surface = gtk::Overlay::new();
        task_surface.set_widget_name("task-surface");
        task_surface.set_child(Some(&scroll));
        // Only the small islands cover content; the space between them belongs
        // to the scroller, including pointer picking and scroll gestures.
        body.set_margin_bottom(64);
        stack.add_named(&task_surface, Some("tasks"));
        let reading = reading::Reading::new(previews);
        stack.add_named(&reading.root, Some("reading"));
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        footer.set_halign(gtk::Align::End);
        footer.set_valign(gtk::Align::End);
        footer.set_margin_bottom(16);
        footer.set_widget_name("task-footer-actions");
        footer.add_css_class("task-footer");
        let edit_island = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        edit_island.add_css_class("task-footer");
        edit_island.set_widget_name("task-footer-edit");
        edit_island.set_halign(gtk::Align::Start);
        edit_island.set_valign(gtk::Align::End);
        edit_island.set_margin_bottom(16);
        task_surface.add_overlay(&edit_island);
        footer.append(&undo);
        task_surface.add_overlay(&footer);
        let view = Rc::new(Self {
            column_count: Cell::new(2),
            root,
            stack,
            body,
            scroll,
            toolbar: bar,
            heading: RefCell::new(heading),
            count,
            exit: exit.clone(),
            domain_order: RefCell::new(vec![]),
            collapsed: RefCell::new(Default::default()),
            expanded: RefCell::new(Default::default()),
            pending_d: RefCell::new(None),
            pending_click: RefCell::new(None),
            drag_scroll: Cell::new(0.),
            preferences: RefCell::new(
                json!({"domainSort":"manual","projectSort":"first-due","doubleDInterval":350,"manualDomainOrderJson":"[]"}),
            ),
            save_preferences: RefCell::new(None),
            options: options.clone(),
            reading,
            message,
            filter,
            sort,
            dates,
            project: RefCell::new(String::new()),
            completed,
            cancelled,
            blocked,
            details,
            state: RefCell::new(json!({})),
            collection: RefCell::new("todo".into()),
            selector: collection.clone(),
            selected: RefCell::new(String::new()),
            send: Rc::new(send),
            pending: Cell::new(false),
            pending_action: RefCell::new(String::new()),
            mutations: RefCell::new(mutations::Mutations::default()),
            motion_epoch: Cell::new(0),
            outbox: RefCell::new(None),
            recover: recover.clone(),
            media_busy: Cell::new(false),
            media_epoch: Cell::new(0),
            dragging: Cell::new(false),
            drag_task: RefCell::new(None),
            move_projection: RefCell::new(None),
            empty_targets,
            rendered: RefCell::new(Value::Null),
            completing: Cell::new(false),
            completion_rows: RefCell::new(Vec::new()),
            markers: RefCell::new(BTreeMap::new()),
            flip_positions: RefCell::new(BTreeMap::new()),
            navigation: RefCell::new(BTreeMap::new()),
            order: RefCell::new(Vec::new()),
            editor: RefCell::new(None),
        });
        let weak = Rc::downgrade(&view);
        recover.connect_clicked(move |_| {
            let Some(view) = weak.upgrade() else { return };
            if view.is_editing() {
                view.error("Finish the open edit before recovering another.");
                return;
            }
            let request = view.mutations.borrow_mut().failed.pop();
            if let Some(request) = request {
                let mut task = view.state.borrow()["tasks"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|t| t["id"] == request["id"]))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                for (key, value) in request["fields"].as_object().unwrap() {
                    task[key] = value.clone();
                }
                task["_recovery"] = json!(true);
                view.edit(task);
                // Keep the saved recovery copy until the replacement edit is queued.
                view.mutations.borrow_mut().failed.push(request);
            }
        });
        let weak = Rc::downgrade(&view);
        view.stack.connect_visible_child_notify(move |_| {
            if let Some(view) = weak.upgrade() {
                view.media_epoch.set(view.media_epoch.get().wrapping_add(1));
                match view.stack.visible_child_name().as_deref() {
                    Some("snooze") => view.options.set_visible(false),
                    Some("tasks") => view.options.set_visible(true),
                    _ => {}
                }
            }
        });
        let weak = Rc::downgrade(&view);
        view.root.connect_unmap(move |_| {
            if let Some(view) = weak.upgrade() {
                view.media_epoch.set(view.media_epoch.get().wrapping_add(1));
            }
        });
        let weak = Rc::downgrade(&view);
        view.reading.connect(move |action, task| {
            if let Some(v) = weak.upgrade() {
                match action {
                    "exit" => exit(),
                    "collection" => {
                        v.selector.set_selected(0);
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
        let edit_selected = button("✎", move || {
            if let Some(v) = weak.upgrade() {
                if let Some(t) = v.selected_task() {
                    v.edit(t);
                }
            }
        });
        edit_selected.set_child(Some(&icons::icon("edit")));
        edit_selected.add_css_class("icon-button");
        edit_selected.set_tooltip_text(Some("Edit selected task"));
        edit_island.append(&edit_selected);
        let dates_help = gtk::MenuButton::new();
        dates_help.set_child(Some(&icons::icon("details")));
        dates_help.set_tooltip_text(Some("Due dates and snoozing"));
        dates_help.add_css_class("icon-button");
        let explanation = gtk::Label::new(Some(
            "Dates show weekdays in the current week, then days, weeks, months or years.
S opens snoozing for the selected task.",
        ));
        explanation.set_margin_top(12);
        explanation.set_margin_bottom(12);
        explanation.set_margin_start(12);
        explanation.set_margin_end(12);
        dates_help.set_popover(Some(&gtk::Popover::builder().child(&explanation).build()));
        footer.append(&dates_help);
        let help = gtk::MenuButton::new();
        help.set_child(Some(&icons::icon("keyboard")));
        help.add_css_class("icon-button");
        let shortcuts = gtk::Label::new(Some(
            "N  New task     /  Search
E  Edit     S  Snooze
D  Complete     D D  Delete
J / K  Navigate     Enter  Expand
Ctrl+Z  Undo     Escape  Back",
        ));
        shortcuts.set_margin_top(12);
        shortcuts.set_margin_bottom(12);
        shortcuts.set_margin_start(12);
        shortcuts.set_margin_end(12);
        help.set_popover(Some(&gtk::Popover::builder().child(&shortcuts).build()));
        footer.append(&help);
        let weak = Rc::downgrade(&view);
        let settings = button("⚙", move || {
            if let Some(v) = weak.upgrade() {
                v.settings();
            }
        });
        settings.set_child(Some(&icons::icon("settings")));
        settings.add_css_class("icon-button");
        footer.append(&settings);
        let weak = Rc::downgrade(&view);
        view.filter.connect_changed(move |_| {
            if let Some(v) = weak.upgrade() {
                v.render();
            }
        });
        let weak = Rc::downgrade(&view);
        view.dates.connect_selected_notify(move |_| {
            if let Some(v) = weak.upgrade() {
                v.domain_order.borrow_mut().clear();
                v.render();
            }
        });
        let weak = Rc::downgrade(&view);
        view.sort.connect_selected_notify(move |_| {
            if let Some(v) = weak.upgrade() {
                if v.sort.selected() == 6 && v.completed.mode() == 0 {
                    v.completed.set_mode(1);
                }
                v.render();
            }
        });
        for filter in [&view.completed, &view.cancelled, &view.blocked] {
            let weak = Rc::downgrade(&view);
            filter.button.connect_clicked(move |_| {
                if let Some(v) = weak.upgrade() {
                    v.render();
                }
            });
        }
        let weak = Rc::downgrade(&view);
        view.details.connect_toggled(move |_| {
            if let Some(v) = weak.upgrade() {
                v.render();
            }
        });
        let weak = Rc::downgrade(&view);
        collection.connect_selected_notify(move |c| {
            if let Some(v) = weak.upgrade() {
                v.project.borrow_mut().clear();
                v.domain_order.borrow_mut().clear();
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
        // Tabs live in the application, and track keyboard/host navigation too.
        for (label, index) in [
            ("All tasks", 0),
            ("This month", 3),
            ("This week", 2),
            ("Today", 1),
        ] {
            let tab = gtk::ToggleButton::with_label(label);
            tab.add_css_class("flat");
            tab.set_active(view.dates.selected() == index);
            let weak = Rc::downgrade(&view);
            tab.connect_toggled(move |t| {
                if let Some(v) = weak.upgrade() {
                    if t.is_active() {
                        v.dates.set_selected(index);
                        v.root.grab_focus();
                    }
                }
            });
            let weak = tab.downgrade();
            view.dates.connect_selected_notify(move |d| {
                if let Some(t) = weak.upgrade() {
                    t.set_active(d.selected() == index);
                }
            });
            navigation.prepend(&tab);
        }
        let weak = Rc::downgrade(&view);
        let search_button = button("⌕", move || {
            if let Some(v) = weak.upgrade() {
                v.filter.set_visible(!WidgetExt::is_visible(&v.filter));
                if WidgetExt::is_visible(&v.filter) {
                    v.filter.grab_focus();
                } else {
                    v.filter.set_text("");
                    v.root.grab_focus();
                }
            }
        });
        controls.insert_child_after(&search_button, Some(&view.sort));
        let weak = Rc::downgrade(&view);
        let projects_tab = button("Projects", move || {
            if let Some(v) = weak.upgrade() {
                v.selector.set_selected(2);
            }
        });
        projects_tab.add_css_class("flat");
        navigation.append(&projects_tab);
        let mut child = controls.first_child();
        while let Some(w) = child {
            if w.is::<gtk::Button>() {
                w.add_css_class("icon-button");
            }
            child = w.next_sibling();
        }
        let weak = Rc::downgrade(&view);
        view.root.add_tick_callback(move |_, _| {
            if let Some(v) = weak.upgrade() {
                if v.dragging.get() && v.drag_scroll.get() != 0. {
                    let a = v.scroll.vadjustment();
                    a.set_value(
                        (a.value() + v.drag_scroll.get())
                            .clamp(a.lower(), (a.upper() - a.page_size()).max(a.lower())),
                    );
                }
                let count = (v.scroll.width() / 420).max(1);
                if v.scroll.width() > 0 && count != v.column_count.replace(count) {
                    *v.rendered.borrow_mut() = Value::Null;
                    v.render();
                }
            }
            glib::ControlFlow::Continue
        });
        let edge = gtk::DropControllerMotion::new();
        let weak = Rc::downgrade(&view);
        edge.connect_motion(move |_, _, y| {
            if let Some(v) = weak.upgrade() {
                v.drag_scroll.set(if y < 44. {
                    -8.
                } else if y > v.scroll.height() as f64 - 44. {
                    8.
                } else {
                    0.
                });
            }
        });
        let weak = Rc::downgrade(&view);
        edge.connect_leave(move |_| {
            if let Some(v) = weak.upgrade() {
                v.drag_scroll.set(0.);
            }
        });
        view.scroll.add_controller(edge);
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&view);
        keys.connect_key_pressed(move |_, key, _, mods| {
            let Some(v) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let page = v.stack.visible_child_name();
            if page.as_deref() != Some("tasks") {
                if key == gdk::Key::Escape
                    && !matches!(page.as_deref(), Some("editor") | Some("reading"))
                {
                    v.stack.set_visible_child_name(
                        if v.collection.borrow().as_str() == "reading" {
                            "reading"
                        } else {
                            "tasks"
                        },
                    );
                    v.focus();
                    return glib::Propagation::Stop;
                }
                return glib::Propagation::Proceed;
            }
            // Editable children own ordinary text and navigation keys.
            if key == gdk::Key::Escape && focus_within(&v.filter) {
                v.filter.set_text("");
                v.filter.set_visible(false);
                v.root.grab_focus();
                return glib::Propagation::Stop;
            }
            if focus_within(&v.filter) && !mods.contains(gdk::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }
            if mods.intersects(gdk::ModifierType::ALT_MASK | gdk::ModifierType::SUPER_MASK) { return glib::Propagation::Proceed; }
            if mods.contains(gdk::ModifierType::CONTROL_MASK) && !matches!(key,gdk::Key::n|gdk::Key::z|gdk::Key::v|gdk::Key::Left) { return glib::Propagation::Proceed; }
            if matches!(key,gdk::Key::Return|gdk::Key::KP_Enter|gdk::Key::space|gdk::Key::d) {
                if let Some(focus) = v.root.root().and_downcast::<gtk::Window>().and_then(|w| gtk::prelude::GtkWindowExt::focus(&w)) {
                    if focus.has_css_class("subtask-row") {
                        if let Some(button)=focus.first_child().and_downcast::<gtk::Button>() {if button.is_sensitive(){button.emit_clicked();}}
                        return glib::Propagation::Stop;
                    }
                }
            }
            match key {
                gdk::Key::Left if mods.contains(gdk::ModifierType::CONTROL_MASK) => {
                    v.project.borrow_mut().clear(); v.domain_order.borrow_mut().clear(); v.render(); v.root.grab_focus();
                }
                gdk::Key::Left | gdk::Key::Right => {
                    v.focus_column(if key == gdk::Key::Left { -1. } else { 1. });
                }
                gdk::Key::slash => {
                    v.filter.set_visible(true);
                    v.filter.grab_focus();
                }
                gdk::Key::Down | gdk::Key::Up | gdk::Key::j | gdk::Key::k | gdk::Key::Tab | gdk::Key::ISO_Left_Tab => {
                    let direction = if matches!(key,gdk::Key::Down|gdk::Key::j) || (key==gdk::Key::Tab && !mods.contains(gdk::ModifierType::SHIFT_MASK)) {1} else {-1};
                    v.focus_vertical(direction);
                }
                gdk::Key::Escape => {
                    if !v.project.borrow().is_empty() {
                        v.project.borrow_mut().clear();
                        v.render();
                    } else {
                        (v.exit)();
                    }
                }
                gdk::Key::N => v.dates.set_selected(1),
                gdk::Key::l => v.dates.set_selected(0),
                gdk::Key::w => v.dates.set_selected(2),
                gdk::Key::m => v.dates.set_selected(3),
                gdk::Key::e | gdk::Key::t | gdk::Key::g => {
                    if let Some(t) = v.selected_task() {
                        v.edit(t);
                    }
                }
                gdk::Key::s => {
                    if let Some(t) = v.selected_task() {
                        v.snooze(t);
                    }
                }
                gdk::Key::c => {
                    if let Some(t) = v.selected_task() {
                        v.action("status", &t, json!({"status":"cancelled"}));
                    }
                }
                gdk::Key::n => {
                    v.edit(json!({"kind":v.collection.borrow().clone(),"title":"","group":"All","project":v.project.borrow().clone()}));
                }
                gdk::Key::v if mods.contains(gdk::ModifierType::CONTROL_MASK) => { if let Some(t)=v.selected_task(){v.paste_image(t);} }
                gdk::Key::z if mods.contains(gdk::ModifierType::CONTROL_MASK) => {
                    v.request(json!({"action":"undo"}))
                }
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    if let Some(t) = v.selected_task() {
                        if s(&t, "kind") == "project" {
                            *v.project.borrow_mut() = s(&t, "id").into();
                            v.render();
                        } else {
                            v.toggle_subtasks(&t);
                        }
                    }
                }
                gdk::Key::d => {
                    if let Some(t) = v.selected_task() {
                        let previous = v.pending_d.borrow_mut().take();
                        let double = previous.as_ref().is_some_and(|(id, _)| id == s(&t, "id"));
                        if let Some((_, timer)) = previous {
                            timer.remove();
                        }
                        if double {
                            v.action("purge", &t, json!({}));
                        } else {
                            let weak = Rc::downgrade(&v);
                            let id = s(&t, "id").to_owned();
                            let timer = glib::timeout_add_local_once(
                                std::time::Duration::from_millis(
                                    v.preferences.borrow()["doubleDInterval"]
                                        .as_u64()
                                        .unwrap_or(350),
                                ),
                                move || {
                                    if let Some(v) = weak.upgrade() {
                                        v.pending_d.borrow_mut().take();
                                        if v.selected.borrow().as_str() == s(&t, "id")
                                            && !v.is_editing()
                                            && !["project", "template"].contains(&s(&t, "kind"))
                                        {
                                            v.action("toggle", &t, json!({}));
                                        }
                                    }
                                },
                            );
                            *v.pending_d.borrow_mut() = Some((id, timer));
                        }
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
    pub fn configure_preferences(self: &Rc<Self>, value: Value, save: impl Fn(&Value) + 'static) {
        *self.preferences.borrow_mut() = value;
        *self.save_preferences.borrow_mut() = Some(Rc::new(save));
        self.domain_order.borrow_mut().clear();
        *self.rendered.borrow_mut() = Value::Null;
        self.render();
    }
    pub(crate) fn preference(self: &Rc<Self>, key: &str, value: Value) {
        self.preferences.borrow_mut()[key] = value;
        if let Some(save) = self.save_preferences.borrow().as_ref() {
            save(&self.preferences.borrow());
        }
        self.domain_order.borrow_mut().clear();
        *self.rendered.borrow_mut() = Value::Null;
        let page = self.stack.visible_child_name();
        self.render();
        if let Some(page) = page {
            self.stack.set_visible_child_name(&page);
        }
    }
    pub fn configure_mutations(
        &self,
        project: impl Fn(&Value, &Value) -> Result<Value, String> + 'static,
    ) {
        self.mutations.borrow_mut().projector = Some(Rc::new(project));
    }
    pub fn configure_outbox(self: &Rc<Self>, path: std::path::PathBuf) -> Result<(), String> {
        if path.exists() {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            self.mutations.borrow_mut().restore(&value)?;
            let visible = self.mutations.borrow().visible();
            self.present(&visible);
        }
        *self.outbox.borrow_mut() = Some(path);
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(view) = weak.upgrade() {
                view.dispatch_next();
            }
        });
        Ok(())
    }
    fn persist_outbox(&self) -> Result<(), String> {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let path = self.outbox.borrow();
        let Some(path) = path.as_ref() else {
            return Ok(());
        };
        let parent = path.parent().ok_or("Invalid outbox directory")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        let temporary = path.with_extension("tmp");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(self.mutations.borrow().snapshot().to_string().as_bytes())
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, path).map_err(|e| e.to_string())?;
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn retry_later(self: &Rc<Self>, message: &str) {
        self.pending.set(false);
        self.message
            .set_text(&format!("Changes queued · {message}"));
        self.message.set_visible(true);
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
            if let Some(view) = weak.upgrade() {
                view.dispatch_next();
            }
        });
    }
    pub fn has_queued_updates(&self) -> bool {
        !self.mutations.borrow().queue.is_empty()
    }
    pub fn update(self: &Rc<Self>, value: &Value) {
        // Unsolicited polling must not overwrite newer local intent.
        if self.has_queued_updates() {
            if value["mutation_accepted"] == true && self.mutations.borrow().projector.is_none() {
                let id = self.mutations.borrow().queue.front().unwrap()["request_id"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned();
                self.response(&id, Ok(value.clone()));
            }
            return;
        }
        self.mutations.borrow_mut().ingest(value);
        self.present(value);
    }
    pub fn response(self: &Rc<Self>, id: &str, result: Result<Value, String>) {
        if let Ok(value) = &result {
            if let Some(message) = value["retryable_error"].as_str() {
                self.retry_later(message);
                return;
            }
        }
        let resolved = self
            .mutations
            .borrow_mut()
            .resolve(id, result.as_ref().ok());
        if !resolved {
            return;
        }
        self.pending.set(false);
        if let Err(message) = self.persist_outbox() {
            self.error(&format!("Could not save update queue: {message}"));
        }
        self.move_projection.borrow_mut().take();
        let mut visible = self.mutations.borrow().visible();
        if let Ok(value) = &result {
            if let Some(source) = value.get("source") {
                visible["source"] = source.clone();
            }
        }
        if let Err(message) = &result {
            self.motion_epoch.set(self.motion_epoch.get() + 1);
            self.completion_rows.borrow_mut().clear();
            if self.completing.replace(false) {
                *self.rendered.borrow_mut() = Value::Null;
            }
            self.present(&visible);
            self.error(message);
        } else {
            self.present(&visible);
        }
        self.dispatch_next();
    }
    fn dispatch_next(self: &Rc<Self>) {
        if self.pending.get() {
            return;
        }
        if let Err(message) = self.persist_outbox() {
            self.error(&format!("Could not save update queue: {message}"));
            return;
        }
        let request = self.mutations.borrow().wire();
        if let Some(request) = request {
            self.pending.set(true);
            *self.pending_action.borrow_mut() = s(&request, "action").to_owned();
            (self.send)(request);
        }
    }
    fn present(self: &Rc<Self>, value: &Value) {
        if let Some(source) = value["source"].as_str() {
            self.pending.set(false);
            self.show_image(source);
            return;
        }
        let newly_completed: Vec<String> = value["tasks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|task| {
                task["done"] == true
                    && self.state.borrow()["tasks"].as_array().is_some_and(|old| {
                        old.iter()
                            .any(|t| t["id"] == task["id"] && t["done"] != true)
                    })
            })
            .map(|t| s(t, "id").to_owned())
            .collect();
        if self.completing.get()
            && value["tasks"].as_array().is_some_and(|rows| {
                rows.iter().any(|t| {
                    t["done"] != true
                        && self.state.borrow()["tasks"].as_array().is_some_and(|old| {
                            old.iter().any(|o| o["id"] == t["id"] && o["done"] == true)
                        })
                })
            })
        {
            self.motion_epoch.set(self.motion_epoch.get() + 1);
            self.completing.set(false);
            self.completion_rows.borrow_mut().clear();
            *self.rendered.borrow_mut() = Value::Null;
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
        if value["mutation_accepted"] == true {
            self.move_projection.borrow_mut().take();
        }
        if value["mutation_accepted"] == true
            || ["refresh", "list", "plans", "open-reference"]
                .contains(&self.pending_action.borrow().as_str())
        {
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
        self.recover
            .set_visible(!self.mutations.borrow().failed.is_empty());
        let message = if s(value, "sync_message").is_empty() {
            s(value, "message")
        } else {
            s(value, "sync_message")
        };
        self.message.set_text(message);
        self.message.set_visible(!message.is_empty());
        self.animate_completions(&newly_completed);
        self.render();
        if !s(value, "draft_id").is_empty() && self.editor.borrow().is_none() {
            self.edit(value["draft"].clone());
        }
    }
    fn animate_completions(self: &Rc<Self>, ids: &[String]) {
        let already_running = self.completing.get();
        let cards: Vec<_> = ids
            .iter()
            .filter_map(|id| {
                let card = self.navigation.borrow().get(id)?.upgrade()?;
                self.markers.borrow().get(id)?.complete();
                Some(card)
            })
            .collect();
        if cards.is_empty() {
            return;
        }
        self.completion_rows.borrow_mut().extend(
            cards
                .into_iter()
                .map(|card| (card, None, self.completed.mode() == 0)),
        );
        if already_running {
            return;
        }
        self.completing.set(true);
        let weak = Rc::downgrade(self);
        let epoch = self.motion_epoch.get();
        self.root.add_tick_callback(move |_, clock| {
            let Some(view) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if view.motion_epoch.get() != epoch {
                return glib::ControlFlow::Break;
            }
            let now = clock.frame_time();
            let mut finished = true;
            for (card, start, fade) in view.completion_rows.borrow_mut().iter_mut() {
                let elapsed = (now - *start.get_or_insert(now)) as f64 / 1000.;
                if *fade {
                    card.set_opacity(1. - ((elapsed - 280.) / 180.).clamp(0., 1.));
                }
                finished &= elapsed >= if *fade { 460. } else { 280. };
            }
            if !finished {
                return glib::ControlFlow::Continue;
            }
            view.completion_rows.borrow_mut().clear();
            *view.flip_positions.borrow_mut() = view
                .navigation
                .borrow()
                .values()
                .filter_map(|w| w.upgrade())
                .filter_map(|w| {
                    w.compute_bounds(&view.scroll)
                        .map(|b| (w.widget_name().to_string(), (b.x(), b.y())))
                })
                .collect();
            view.completing.set(false);
            view.render();
            glib::ControlFlow::Break
        });
    }
    pub fn reject_pending(self: &Rc<Self>, message: &str) {
        let id = self
            .mutations
            .borrow()
            .queue
            .front()
            .and_then(|r| r["request_id"].as_str())
            .map(str::to_owned);
        if let Some(id) = id {
            self.response(&id, Err(message.into()));
        } else {
            self.error(message);
        }
    }
    pub fn error(self: &Rc<Self>, message: &str) {
        let moved = self.move_projection.borrow_mut().take().is_some();
        if moved {
            *self.rendered.borrow_mut() = Value::Null;
            self.render();
        }
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
    /// Navigate from a host without replacing an in-progress edit.
    pub fn open(&self, collection: &str, view: &str) {
        if self.is_editing() {
            return;
        }
        self.selector.set_selected(
            ["todo", "reading", "project", "template"]
                .iter()
                .position(|s| *s == collection)
                .unwrap_or(0) as u32,
        );
        self.dates.set_selected(match view {
            "today" => 1,
            "week" => 2,
            "month" => 3,
            _ => 0,
        });
    }
    pub fn is_editing(&self) -> bool {
        self.stack.visible_child_name().as_deref() == Some("editor")
    }
    pub fn focus(&self) {
        if self.collection.borrow().as_str() == "reading" {
            self.reading.root.grab_focus();
            return;
        }
        if let Some((id, _, _)) = self.order.borrow().first() {
            let id = id.clone();
            *self.selected.borrow_mut() = id.clone();
            let card = self.navigation.borrow().get(&id).and_then(|w| w.upgrade());
            if let Some(card) = card {
                card.grab_focus();
                return;
            }
        }
        self.root.grab_focus();
    }
    pub(crate) fn request(self: &Rc<Self>, mut request: Value) {
        self.message.set_text("");
        self.message.set_visible(false);
        if request["request_id"].is_null() {
            request["request_id"] = json!(uuid::Uuid::new_v4().to_string());
        }
        if ["create", "reading-save"].contains(&s(&request, "action")) {
            request["_local_id"] = json!(uuid::Uuid::new_v4().to_string());
        }
        let recover_id = request["id"].clone();
        let saving = ["create", "edit", "reading-save"].contains(&s(&request, "action"));
        let queued = self.mutations.borrow_mut().enqueue(request);
        if let Err(message) = queued {
            self.error(&message);
            return;
        }
        if saving {
            self.mutations
                .borrow_mut()
                .failed
                .retain(|r| r["id"] != recover_id);
        }
        if let Err(message) = self.persist_outbox() {
            self.error(&format!("Could not save update queue: {message}"));
            return;
        }
        if saving && self.is_editing() {
            self.close_editor();
        }
        let visible = self.mutations.borrow().visible();
        if self.mutations.borrow().projector.is_some() {
            self.move_projection.borrow_mut().take();
        }
        self.present(&visible);
        self.dispatch_next();
    }

    fn selected_task(&self) -> Option<Value> {
        self.state.borrow()["tasks"]
            .as_array()?
            .iter()
            .find(|t| s(t, "id") == self.selected.borrow().as_str())
            .cloned()
    }
    fn action(self: &Rc<Self>, action: &str, task: &Value, extra: Value) {
        let mut r = json!({"action":action,"id":task["id"],"revision":task["revision"],"period":task["period"],"actor":"user"});
        if let Some(map) = extra.as_object() {
            for (k, v) in map {
                r[k] = v.clone();
            }
        }
        if !self.dragging.get() {
            mutations::Mutations::revision(&self.state.borrow(), &mut r);
        }
        self.request(r);
    }
    pub fn refresh(self: &Rc<Self>) {
        if self.has_queued_updates() {
            self.dispatch_next();
            return;
        }
        if !self.pending.get() && !self.is_editing() {
            self.request(json!({"action":"refresh"}));
        }
    }
    pub fn render(self: &Rc<Self>) {
        // Keep an in-progress editor and its input focus intact during sync updates.
        if self.is_editing()
            || self.completing.get()
            || self.stack.visible_child_name().as_deref() == Some("snooze")
        {
            return;
        }
        if self.dragging.get() {
            return;
        }
        let signature = json!([
            visual_tasks(&self.state.borrow()["tasks"]),
            *self.move_projection.borrow(),
            self.collection.borrow().as_str(),
            self.filter.text().as_str(),
            self.sort.selected(),
            self.dates.selected(),
            self.project.borrow().as_str(),
            self.completed.mode(),
            self.cancelled.mode(),
            self.blocked.mode(),
            self.details.is_active(),
            *self.collapsed.borrow()
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
        self.markers.borrow_mut().clear();
        self.navigation.borrow_mut().clear();
        self.order.borrow_mut().clear();
        clear(&self.body);
        let mut state = self.state.borrow().clone();
        if let Some(projection) = self.move_projection.borrow().as_ref() {
            if let Some(tasks) = state["tasks"].as_array_mut() {
                if let Some(task) = tasks.iter_mut().find(|t| t["id"] == projection["id"]) {
                    task["group"] = projection["group"].clone();
                    if projection["project"].is_string() {
                        task["project"] = projection["project"].clone();
                    }
                }
            }
        }
        let collection = self.collection.borrow().clone();
        let filter = self.filter.text().to_lowercase();
        let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let horizon = presentation::horizon(s(&state, "today"), self.dates.selected());
        for task in state["tasks"].as_array().into_iter().flatten() {
            let kind = s(task, "kind");
            if kind != collection && !(collection == "todo" && kind == "project") {
                continue;
            }
            if !self.project.borrow().is_empty() {
                if s(task, "project") != self.project.borrow().as_str() {
                    continue;
                }
            } else if !s(task, "project").is_empty()
                && state["tasks"].as_array().is_some_and(|rows| {
                    rows.iter()
                        .any(|p| p["id"] == task["project"] && p["onlyInProject"] == true)
                })
            {
                continue;
            }
            let due = if !s(task, "effectiveDue").is_empty() {
                s(task, "effectiveDue")
            } else {
                s(task, "due")
            };
            if kind != "project"
                && kind != "template"
                && self.dates.selected() > 0
                && !due.is_empty()
                && due > horizon.as_str()
            {
                continue;
            }
            if !status_filter::matches(
                [
                    self.completed.mode(),
                    self.cancelled.mode(),
                    self.blocked.mode(),
                ],
                [
                    task["done"] == true,
                    s(task, "status") == "cancelled",
                    s(task, "status") == "blocked" || task["blocked"] == true,
                ],
            ) {
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
        // Recreate the heading after a page transition. Some renderers retain a
        // stale text node when an identically styled editor heading is disposed.
        let heading = gtk::Label::builder()
            .label(match collection.as_str() {
                "project" => "Projects",
                "template" => "Templates",
                _ => "Tasks",
            })
            .xalign(0.)
            .build();
        heading.add_css_class("task-heading");
        if let Some(parent) = self.heading.borrow().parent().and_downcast::<gtk::Box>() {
            parent.insert_child_after(&heading, None::<&gtk::Widget>);
            parent.remove(&*self.heading.borrow());
        }
        *self.heading.borrow_mut() = heading;
        self.count.set_text(&format!(
            "{} tasks · {}",
            groups.values().map(Vec::len).sum::<usize>(),
            ["All dates", "Today", "This week", "This month"][self.dates.selected() as usize]
        ));
        self.toolbar.queue_draw();
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
        let columns = gtk::Box::new(gtk::Orientation::Horizontal, 32);
        columns.set_homogeneous(true);
        self.body.append(&columns);
        let lanes: Vec<_> = (0..self.column_count.get())
            .map(|_| {
                let lane = gtk::Box::new(gtk::Orientation::Vertical, 28);
                lane.set_hexpand(true);
                lane.set_valign(gtk::Align::Start);
                columns.append(&lane);
                lane
            })
            .collect();
        // Existing domains retain their slots until navigation changes the view.
        let mut order = self.domain_order.borrow_mut();
        if order.is_empty() {
            let prefs = self.preferences.borrow();
            let mut names: Vec<String> = groups.keys().cloned().collect();
            match s(&prefs, "domainSort") {
                "due" => names.sort_by_key(|group| {
                    groups[group]
                        .iter()
                        .filter_map(|t| {
                            let d = s(t, "effectiveDue");
                            let d = if d.is_empty() { s(t, "due") } else { d };
                            (!d.is_empty()).then(|| d.to_owned())
                        })
                        .min()
                        .unwrap_or_else(|| "9999".into())
                }),
                "alphabetical" => {}
                _ => {
                    let mut manual: Vec<String> =
                        serde_json::from_str(s(&prefs, "manualDomainOrderJson"))
                            .unwrap_or_default();
                    if manual.is_empty() {
                        manual = state["groups"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|g| g.as_str().map(str::to_owned))
                            .collect();
                    }
                    names.sort_by_key(|g| manual.iter().position(|n| n == g).unwrap_or(usize::MAX));
                }
            }
            order.extend(names);
        }
        for group in groups.keys() {
            if !order.contains(group) {
                order.push(group.clone());
            }
        }
        let ordered = order.clone();
        drop(order);
        for (slot, group) in ordered.into_iter().enumerate() {
            let mut tasks = groups.remove(&group).unwrap_or_default();
            if tasks.is_empty() {
                continue;
            }
            let sort = self.sort.selected();
            tasks.sort_by(|a, b| {
                if sort == 6 {
                    return (b["done"] == true)
                        .cmp(&(a["done"] == true))
                        .then_with(|| {
                            presentation::completion_time(b).cmp(&presentation::completion_time(a))
                        })
                        .then(s(a, "title").cmp(s(b, "title")));
                }
                let project_sort = s(&self.preferences.borrow(), "projectSort").to_owned();
                if project_sort != "first-due"
                    && (s(a, "kind") == "project") != (s(b, "kind") == "project")
                {
                    let order = (s(b, "kind") == "project").cmp(&(s(a, "kind") == "project"));
                    return if project_sort == "last" {
                        order.reverse()
                    } else {
                        order
                    };
                }
                let key = match sort {
                    0 | 1 => "effectiveDue",
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
                let sort_value = |t: &Value| {
                    let v = s(t, key);
                    if v.is_empty() && key == "effectiveDue" {
                        s(t, "due").to_owned()
                    } else {
                        v.to_owned()
                    }
                };
                if (sort == 0 || sort == 1) && sort_value(a).is_empty() != sort_value(b).is_empty()
                {
                    return sort_value(a).is_empty().cmp(&sort_value(b).is_empty());
                }
                let order = sort_value(a)
                    .cmp(&sort_value(b))
                    .then(s(a, "title").cmp(s(b, "title")));
                if sort == 1 || sort == 3 {
                    order.reverse()
                } else {
                    order
                }
            });
            if self.sort.selected() == 4 {
                if let Some(projection) = self.move_projection.borrow().as_ref() {
                    if let Some(index) = tasks.iter().position(|t| t["id"] == projection["id"]) {
                        let task = tasks.remove(index);
                        let index = tasks
                            .iter()
                            .position(|t| t["id"] == projection["before_id"])
                            .unwrap_or(tasks.len());
                        tasks.insert(index, task);
                    }
                }
            }
            let heading = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let label = gtk::Label::builder()
                .label(if group == "All" { "Inbox" } else { &group })
                .xalign(0.0)
                .build();
            let weak = Rc::downgrade(self);
            let name = group.clone();
            let collapsed = self.collapsed.borrow().contains(&group);
            heading.append(&button(if collapsed { "›" } else { "⌄" }, move || {
                if let Some(v) = weak.upgrade() {
                    let mut set = v.collapsed.borrow_mut();
                    if !set.remove(&name) {
                        set.insert(name.clone());
                    }
                    drop(set);
                    v.render();
                }
            }));
            label.add_css_class("domain-heading");
            heading.append(&label);
            let count = gtk::Label::new(Some(&tasks.len().to_string()));
            count.add_css_class("muted");
            heading.append(&count);
            let rename = gtk::GestureClick::new();
            let weak = Rc::downgrade(self);
            let name = group.clone();
            rename.connect_pressed(move |_, presses, _, _| {
                if presses == 2 {
                    if let Some(v) = weak.upgrade() {
                        v.rename_group(&name);
                    }
                }
            });
            label.add_controller(rename);
            heading.add_css_class("domain-bar");
            let panel = gtk::Box::new(gtk::Orientation::Vertical, 16);
            panel.set_margin_top(10);
            panel.set_valign(gtk::Align::Start);
            let weak = Rc::downgrade(self);
            let name = group.clone();
            heading.append(&button("+", move || { if let Some(v)=weak.upgrade() { v.edit(json!({"kind":v.collection.borrow().clone(),"title":"","group":name,"project":v.project.borrow().clone()})); } }));
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
            list.animate_from(self.flip_positions.borrow().clone());
            self.order.borrow_mut().extend(
                tasks
                    .iter()
                    .enumerate()
                    .filter(|_| !collapsed)
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
                let task = v.drag_task.borrow().clone().filter(|t| s(t, "id") == id);
                if let Some(t) = task {
                    v.drop_task(&t, &name, "", None);
                    true
                } else {
                    false
                }
            });
            panel.add_controller(drop);
            list.set_visible(!collapsed);
            panel.append(&list);
            lanes[slot % lanes.len()].append(&panel);
        }
        self.flip_positions.borrow_mut().clear();
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
    fn drop_task(self: &Rc<Self>, task: &Value, group: &str, before: &str, project: Option<&str>) {
        let before = if self.sort.selected() == 4 {
            before
        } else {
            ""
        };
        *self.move_projection.borrow_mut() =
            Some(json!({"id":task["id"],"group":group,"before_id":before,"project":project}));
        *self.selected.borrow_mut() = s(task, "id").to_owned();
        if let Some(project) = project {
            self.action(
                "edit",
                task,
                json!({"fields":{"group":group,"project":project}}),
            );
        } else {
            self.action("move", task, json!({"group":group,"before_id":before}));
        }
    }
    fn show_empty_targets(self: &Rc<Self>) {
        clear(&self.empty_targets);
        let state = self.state.borrow();
        for group in state["groups"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if state["tasks"]
                .as_array()
                .is_some_and(|ts| ts.iter().any(|t| s(t, "group") == group))
            {
                continue;
            }
            let label = gtk::Label::new(Some(group));
            label.add_css_class("empty-drop-target");
            let target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
            let weak = Rc::downgrade(self);
            let group = group.to_owned();
            target.connect_drop(move |_, value, _, _| {
                let Some(v) = weak.upgrade() else {
                    return false;
                };
                let Ok(id) = value.get::<String>() else {
                    return false;
                };
                let task = v.drag_task.borrow().clone();
                if let Some(task) = task.filter(|t| s(t, "id") == id) {
                    v.drop_task(&task, &group, "", None);
                    true
                } else {
                    false
                }
            });
            label.add_controller(target);
            self.empty_targets.append(&label);
        }
        self.empty_targets
            .set_visible(self.empty_targets.first_child().is_some());
    }
    /// Column-major screen order, independent of domain insertion order.
    fn visual_rows(&self) -> Vec<(String, f32, f32)> {
        let mut rows: Vec<_> = self
            .order
            .borrow()
            .iter()
            .filter_map(|(id, column, index)| {
                let column = column.upgrade()?;
                let bounds = column.compute_bounds(&self.scroll)?;
                Some((
                    id.clone(),
                    bounds.x(),
                    bounds.y() + column.row_offset(*index),
                ))
            })
            .collect();
        rows.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.2.total_cmp(&b.2)));
        rows
    }
    fn focus_vertical(&self, direction: i32) {
        let mut targets: Vec<(String, Option<gtk::Widget>)> = Vec::new();
        for (id, _, _) in self.visual_rows() {
            targets.push((id.clone(), None));
            if !self.expanded.borrow().contains(&id) {
                continue;
            }
            let card = self.navigation.borrow().get(&id).and_then(|w| w.upgrade());
            let mut child = card.and_then(|c| c.first_child());
            while let Some(widget) = child {
                if widget.widget_name() == "task-subtasks" {
                    let mut node = widget.first_child();
                    while let Some(row) = node {
                        if row.has_css_class("subtask-row") && row.is_focusable() {
                            targets.push((id.clone(), Some(row.clone())));
                        }
                        node = row.next_sibling();
                    }
                }
                child = widget.next_sibling();
            }
        }
        if targets.is_empty() {
            return;
        }
        let focus = self
            .root
            .root()
            .and_downcast::<gtk::Window>()
            .and_then(|w| gtk::prelude::GtkWindowExt::focus(&w));
        let at = targets
            .iter()
            .position(|(_, row)| row.as_ref().is_some_and(|r| Some(r) == focus.as_ref()))
            .or_else(|| {
                targets
                    .iter()
                    .position(|(id, row)| row.is_none() && *id == *self.selected.borrow())
            });
        let index = at
            .map(|at| (at as i32 + direction).rem_euclid(targets.len() as i32) as usize)
            .unwrap_or(0);
        let (id, row) = &targets[index];
        if let Some(row) = row {
            row.grab_focus();
            if let Some(bounds) = row.compute_bounds(&self.scroll) {
                let a = self.scroll.vadjustment();
                if bounds.y() < 0. {
                    a.set_value(a.value() + bounds.y() as f64);
                } else if bounds.y() + bounds.height() > self.scroll.height() as f32 {
                    a.set_value(
                        a.value()
                            + (bounds.y() + bounds.height() - self.scroll.height() as f32) as f64,
                    );
                }
            }
        } else {
            self.focus_task(id);
        }
    }
    fn focus_column(&self, direction: f32) {
        let rows = self.visual_rows();
        let selected = self.selected.borrow().clone();
        let Some((_, x, y)) = rows.iter().find(|(id, _, _)| *id == selected) else {
            return;
        };
        let distance = rows
            .iter()
            .filter(|(_, cx, _)| (*cx - *x) * direction > 1.)
            .map(|(_, cx, _)| (*cx - *x).abs())
            .min_by(f32::total_cmp);
        let Some(distance) = distance else {
            return;
        };
        let target = rows
            .iter()
            .filter(|(_, cx, _)| {
                (*cx - *x) * direction > 1. && ((*cx - *x).abs() - distance).abs() < 1.
            })
            .min_by(|a, b| (a.2 - *y).abs().total_cmp(&(b.2 - *y).abs()));
        if let Some((id, _, _)) = target {
            self.focus_task(id);
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
        let card = gtk::Box::new(gtk::Orientation::Vertical, 5);
        card.set_widget_name(&format!("task-{}", s(&task, "id")));
        card.add_css_class("card");
        card.add_css_class("task-card");
        card.set_focusable(true);
        if s(&task, "id") == self.selected.borrow().as_str() {
            card.add_css_class("task-selected");
        }
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.add_css_class("task-title-row");
        let snoozed_date = chrono::DateTime::parse_from_rfc3339(s(&task, "snoozedUntil"))
            .map(|date| date.with_timezone(&chrono::Local).date_naive().to_string())
            .unwrap_or_default();
        let due = if !snoozed_date.is_empty() {
            snoozed_date.as_str()
        } else if s(&task, "effectiveDue").is_empty() {
            s(&task, "due")
        } else {
            s(&task, "effectiveDue")
        };
        let weak = Rc::downgrade(self);
        let t = task.clone();
        let marker = marker::Marker::new(
            &presentation::date_code(due, s(&self.state.borrow(), "today"), self.dates.selected()),
            task["done"] == true,
            21,
            move || {
                if let Some(v) = weak.upgrade() {
                    v.action("toggle", &t, json!({}));
                }
            },
        );
        self.markers
            .borrow_mut()
            .retain(|_, marker| marker.button.parent().is_some());
        self.markers
            .borrow_mut()
            .insert(s(&task, "id").to_owned(), marker.clone());
        marker
            .button
            .set_widget_name(&format!("complete-{}", s(&task, "id")));
        marker
            .button
            .set_sensitive(!["project", "template"].contains(&s(&task, "kind")));
        if !snoozed_date.is_empty() {
            marker.set_snoozed(s(&task, "snoozedUntil"));
        }
        marker.set_selected(s(&task, "id") == *self.selected.borrow());
        marker.follow_focus(&card);
        marker.hover_on(&row);
        row.append(&marker.button);
        let title = gtk::Label::builder()
            .label(s(&task, "title"))
            .xalign(0.0)
            .wrap(true)
            .hexpand(true)
            .build();
        title.set_valign(gtk::Align::Start);
        title.add_css_class("task-title");
        if task["done"] == true || s(&task, "status") == "cancelled" {
            let attributes = gtk::pango::AttrList::new();
            attributes.insert(gtk::pango::AttrInt::new_strikethrough(true));
            title.set_attributes(Some(&attributes));
            card.add_css_class(if task["done"] == true {
                "task-completed"
            } else {
                "task-cancelled"
            });
        }
        row.append(&title);
        card.append(&row);
        if s(&task, "kind") == "project" {
            let (completed, total) =
                presentation::project_progress(&task, &self.state.borrow(), self.dates.selected());
            let progress = gtk::DrawingArea::builder()
                .content_height(4)
                .hexpand(true)
                .build();
            progress.set_widget_name(&format!("project-progress-{}", s(&task, "id")));
            progress.set_margin_start(36);
            progress.set_margin_top(5);
            progress.set_margin_bottom(5);
            let label = format!("Project progress: {completed} of {total} complete");
            progress.set_tooltip_text(Some(&label));
            progress.update_property(&[gtk::accessible::Property::Label(&label)]);
            progress.set_draw_func(move |_, c, width, _| {
                let width = width as f64 * 0.7;
                c.set_line_width(4.);
                c.set_line_cap(gtk::cairo::LineCap::Round);
                c.set_source_rgb(57. / 255., 53. / 255., 75. / 255.);
                c.move_to(2., 2.);
                c.line_to((width - 2.).max(2.), 2.);
                let _ = c.stroke();
                if completed > 0 && total > 0 {
                    c.set_source_rgb(168. / 255., 199. / 255., 160. / 255.);
                    c.move_to(2., 2.);
                    c.line_to((width * completed as f64 / total as f64 - 2.).max(2.), 2.);
                    let _ = c.stroke();
                }
            });
            card.append(&progress);
        }

        if self.details.is_active() && !s(&task, "reference").is_empty() {
            let details = gtk::Label::builder()
                .label(s(&task, "reference"))
                .xalign(0.)
                .wrap(true)
                .build();
            details.add_css_class("muted");
            details.set_margin_start(36);
            card.append(&details);
        }
        let tags = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        tags.set_margin_start(36);
        for tag in task["tags"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let weak = Rc::downgrade(self);
            let text = tag.to_owned();
            let chip = button(&format!("#{tag}"), move || {
                if let Some(v) = weak.upgrade() {
                    v.filter.set_visible(true);
                    v.filter.set_text(&text);
                }
            });
            chip.add_css_class("task-tag");
            tags.append(&chip);
        }
        if tags.first_child().is_some() {
            card.append(&tags);
        }
        if task["workflowRows"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())
        {
            let pips = gtk::FlowBox::new();
            pips.set_selection_mode(gtk::SelectionMode::None);
            pips.set_min_children_per_line(1);
            pips.set_max_children_per_line(100);
            pips.set_column_spacing(5);
            pips.set_row_spacing(5);
            pips.set_halign(gtk::Align::Start);
            pips.set_can_target(false);
            let records = task["subtaskPips"]
                .as_array()
                .or_else(|| task["workflowRows"].as_array());
            for pip in records.into_iter().flatten() {
                let area = gtk::DrawingArea::builder()
                    .content_width(6)
                    .content_height(6)
                    .build();
                let status = s(pip, "status").to_owned();
                area.set_draw_func(move |_, c, _, _| {
                    let alpha = if ["blocked", "cancelled"].contains(&status.as_str()) {
                        0.4
                    } else {
                        1.
                    };
                    let color = if status == "done" {
                        (168., 199., 160.)
                    } else if status == "cancelled" {
                        (235., 111., 146.)
                    } else {
                        (170., 165., 186.)
                    };
                    c.set_source_rgba(color.0 / 255., color.1 / 255., color.2 / 255., alpha);
                    c.set_line_width(1.);
                    c.arc(3., 3., 2.5, 0., std::f64::consts::TAU);
                    if status == "done" || status == "cancelled" {
                        let _ = c.fill_preserve();
                    }
                    let _ = c.stroke();
                });
                pips.insert(&area, -1);
            }
            let weak = Rc::downgrade(self);
            let task = task.clone();
            let toggle = button("", move || {
                if let Some(v) = weak.upgrade() {
                    v.toggle_subtasks(&task);
                }
            });
            toggle.set_child(Some(&pips));
            toggle.add_css_class("subtask-pips");
            toggle.set_margin_start(36);
            toggle.set_tooltip_text(Some("Show or hide subtasks"));
            card.append(&toggle);
        }
        let children = gtk::Box::new(gtk::Orientation::Vertical, 2);
        children.set_widget_name("task-subtasks");
        let expanded = self.expanded.borrow().contains(s(&task, "id"));
        if expanded {
            self.subtasks(&children, &task);
        }
        children.set_visible(expanded);
        card.append(&children);
        if s(&task, "kind") == "todo"
            && !task["workflowRows"]
                .as_array()
                .is_some_and(|rows| !rows.is_empty())
        {
            let weak = Rc::downgrade(self);
            let task = task.clone();
            let add = button("+", move || {
                if let Some(v) = weak.upgrade() {
                    v.add_subtask(&task);
                }
            });
            add.add_css_class("add-subtask");
            add.set_tooltip_text(Some("Add subtask"));
            add.set_valign(gtk::Align::Start);
            add.set_opacity(0.);
            row.append(&add);
            let hover = gtk::EventControllerMotion::new();
            let a = add.clone();
            hover.connect_enter(move |_, _, _| a.set_opacity(1.));
            hover.connect_leave(move |_| add.set_opacity(0.));
            row.add_controller(hover);
        }
        let source = gtk::DragSource::new();
        source.set_actions(gdk::DragAction::MOVE);
        let id = s(&task, "id").to_owned();
        source
            .connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&id.to_value())));
        let weak = Rc::downgrade(self);
        let drag_id = s(&task, "id").to_owned();
        let ghost = card.downgrade();
        source.connect_drag_begin(move |source, _| {
            if let Some(card) = ghost.upgrade() {
                source.set_icon(Some(&gtk::WidgetPaintable::new(Some(&card))), 0, 0);
                card.set_opacity(0.5);
            }
            if let Some(v) = weak.upgrade() {
                if let Some(timer) = v.pending_click.borrow_mut().take() {
                    timer.remove();
                }
                v.dragging.set(true);
                *v.drag_task.borrow_mut() = v.state.borrow()["tasks"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|t| s(t, "id") == drag_id))
                    .cloned();
                v.show_empty_targets();
                v.pin_task(&drag_id, true);
            }
        });
        let weak = Rc::downgrade(self);
        let drag_id = s(&task, "id").to_owned();
        let ghost = card.downgrade();
        source.connect_drag_end(move |_, _, _| {
            if let Some(card) = ghost.upgrade() {
                card.set_opacity(1.);
            }
            if let Some(v) = weak.upgrade() {
                v.drag_scroll.set(0.);
                v.dragging.set(false);
                v.empty_targets.set_visible(false);
                v.drag_task.borrow_mut().take();
                v.pin_task(&drag_id, false);
                v.render();
            }
        });
        card.add_controller(source);
        let weak = Rc::downgrade(self);
        let target = task.clone();
        let drop = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
        drop.connect_enter(|target, _, _| {
            if let Some(w) = target.widget() {
                w.add_css_class("drop-target");
            }
            gdk::DragAction::MOVE
        });
        drop.connect_leave(|target| {
            if let Some(w) = target.widget() {
                w.remove_css_class("drop-target");
            }
        });
        drop.connect_drop(move |drop, value, _, y| {
            let Ok(id) = value.get::<String>() else {
                return false;
            };
            let Some(v) = weak.upgrade() else {
                return false;
            };
            let source = v.drag_task.borrow().clone();
            let Some(t) = source.filter(|t| s(t, "id") == id && t["id"] != target["id"]) else {
                return false;
            };
            if s(&target, "kind") == "project" && s(&t, "kind") == "todo" {
                v.drop_task(&t, s(&target, "group"), "", Some(s(&target, "id")));
            } else {
                let after = drop.widget().is_some_and(|w| y > w.height() as f64 / 2.);
                let before = if after {
                    let order = v.order.borrow();
                    order
                        .iter()
                        .position(|(id, _, _)| id == s(&target, "id"))
                        .and_then(|index| order.get(index + 1))
                        .map(|(id, _, _)| id.clone())
                        .filter(|id| {
                            v.state.borrow()["tasks"].as_array().is_some_and(|ts| {
                                ts.iter()
                                    .any(|t| s(t, "id") == id && t["group"] == target["group"])
                            })
                        })
                        .unwrap_or_default()
                } else {
                    s(&target, "id").to_owned()
                };
                v.drop_task(&t, s(&target, "group"), &before, None);
            }
            true
        });
        card.add_controller(drop);
        let click = gtk::GestureClick::new();
        let weak = Rc::downgrade(self);
        let id = s(&task, "id").to_owned();
        click.connect_pressed(move |gesture, presses, x, y| {
            if let Some(owner) = gesture.widget() {
                let mut picked = owner.pick(x, y, gtk::PickFlags::DEFAULT);
                while let Some(widget) = picked {
                    if widget == owner {
                        break;
                    }
                    if widget.is::<gtk::Button>() || widget.has_css_class("subtask-row") {
                        return;
                    }
                    picked = widget.parent();
                }
            }
            if let Some(v) = weak.upgrade() {
                *v.selected.borrow_mut() = id.clone();
                if let Some(timer) = v.pending_click.borrow_mut().take() {
                    timer.remove();
                }
                if presses == 2 {
                    if let Some(task) = v.selected_task() {
                        v.edit(task);
                    }
                    return;
                }
                if let Some(widget) = gesture.widget() {
                    widget.grab_focus();
                }
                let weak = Rc::downgrade(&v);
                let id = id.clone();
                *v.pending_click.borrow_mut() = Some(glib::timeout_add_local_once(
                    std::time::Duration::from_millis(300),
                    move || {
                        if let Some(v) = weak.upgrade() {
                            v.pending_click.borrow_mut().take();
                            if v.dragging.get() || v.is_editing() || *v.selected.borrow() != id {
                                return;
                            }
                            let task = v.state.borrow()["tasks"]
                                .as_array()
                                .and_then(|ts| ts.iter().find(|t| s(t, "id") == id))
                                .cloned();
                            if let Some(task) = task {
                                if s(&task, "kind") == "project" {
                                    *v.project.borrow_mut() = id;
                                    v.selector.set_selected(0);
                                    v.render();
                                } else {
                                    v.toggle_subtasks(&task);
                                }
                            }
                        }
                    },
                ));
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
    // Expanding one row must not discard the board, its measured heights, or focus.
    fn toggle_subtasks(self: &Rc<Self>, task: &Value) {
        if !task["workflowRows"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())
        {
            return;
        }
        let id = s(task, "id");
        let card = self.navigation.borrow().get(id).and_then(|w| w.upgrade());
        let Some(card) = card else {
            return;
        };
        let mut child = card.first_child();
        while let Some(widget) = child {
            if widget.widget_name() == "task-subtasks" {
                let children = widget.downcast::<gtk::Box>().unwrap();
                let expanded = {
                    let mut set = self.expanded.borrow_mut();
                    if set.remove(id) {
                        false
                    } else {
                        set.insert(id.to_owned());
                        true
                    }
                };
                if expanded && children.first_child().is_none() {
                    self.subtasks(&children, task);
                }
                children.set_visible(expanded);
                return;
            }
            child = widget.next_sibling();
        }
    }
    fn add_subtask(self: &Rc<Self>, task: &Value) {
        let mut draft = task.clone();
        if !draft["subtasks"].is_array() {
            draft["subtasks"] = json!([]);
        }
        draft["subtasks"].as_array_mut().unwrap().push(json!({"id":uuid::Uuid::new_v4().to_string(),"kind":"task","title":"","done":false,"items":[]}));
        self.edit(draft);
    }
    fn subtasks(self: &Rc<Self>, parent: &gtk::Box, task: &Value) {
        for node in task["workflowRows"].as_array().into_iter().flatten() {
            if (s(node, "status") == "blocked" || node["blocked"] == true)
                && self.blocked.mode() == 0
            {
                continue;
            }
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.set_widget_name(&format!("subtask-{}", s(node, "id")));
            row.add_css_class("subtask-row");
            row.set_focusable(true);
            let actionable = !["flow", "group"].contains(&s(node, "kind"))
                && node["blocked"] != true
                && !["blocked", "cancelled"].contains(&s(node, "status"));
            row.set_focusable(actionable);
            let weak = Rc::downgrade(self);
            let parent_task = task.clone();
            let id = node["id"].clone();
            let marker = marker::Marker::new(
                &presentation::date_code(
                    s(node, "due"),
                    s(&self.state.borrow(), "today"),
                    self.dates.selected(),
                ),
                s(node, "status") == "done",
                24,
                move || {
                    if let Some(v) = weak.upgrade() {
                        v.action("subtask-toggle", &parent_task, json!({"subtask_id":id}));
                    }
                },
            );
            marker
                .button
                .set_widget_name(&format!("subtask-complete-{}", s(node, "id")));
            marker.button.set_sensitive(actionable);
            marker.follow_focus(&row);
            marker.hover_on(&marker.button);
            row.append(&marker.button);
            let symbol = gtk::Label::new(Some(if ["flow", "group"].contains(&s(node, "kind")) {
                "⑂"
            } else {
                "•"
            }));
            symbol.add_css_class(match s(node, "kind") {
                "exit" => "subtask-exit",
                "reminder" => "subtask-reminder",
                _ => "subtask-symbol",
            });
            symbol.set_width_chars(1);
            symbol.set_valign(gtk::Align::Start);
            symbol.set_margin_start(node["depth"].as_i64().unwrap_or(0) as i32 * 12);
            row.append(&symbol);
            let label = gtk::Label::builder()
                .label(s(node, "title"))
                .xalign(0.)
                .hexpand(true)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .width_chars(1)
                .build();
            label.add_css_class("subtask-title");
            label.set_valign(gtk::Align::Start);
            let chosen = s(node, "status") == "done"
                && s(node, "kind") != "reminder"
                && (!s(node, "blockId").is_empty()
                    || task["workflowRows"].as_array().is_some_and(|rows| {
                        rows.iter().any(|group| {
                            s(group, "kind") == "group"
                                && group["items"].as_array().is_some_and(|items| {
                                    items.iter().any(|item| item["id"] == node["id"])
                                })
                        })
                    }));
            let flow_picked = s(node, "kind") == "flow"
                && task["workflowRows"].as_array().is_some_and(|rows| {
                    rows.iter().any(|child| {
                        child["blockId"] == node["id"]
                            && s(child, "kind") != "reminder"
                            && s(child, "status") == "done"
                    })
                });
            let attrs = gtk::pango::AttrList::new();
            if s(node, "status") == "cancelled"
                || flow_picked
                || (s(node, "status") == "done" && !chosen)
            {
                attrs.insert(gtk::pango::AttrInt::new_strikethrough(true));
            }
            if chosen || ["flow", "group"].contains(&s(node, "kind")) {
                attrs.insert(gtk::pango::AttrInt::new_weight(gtk::pango::Weight::Bold));
            }
            label.set_attributes(Some(&attrs));
            if ["blocked", "cancelled", "snoozed"].contains(&s(node, "status")) {
                label.add_css_class("muted");
            }
            row.append(&label);
            let click = gtk::GestureClick::new();
            let target = row.downgrade();
            click.connect_released(move |_, _, _, _| {
                if let Some(row) = target.upgrade() {
                    row.grab_focus();
                }
            });
            row.add_controller(click);
            parent.append(&row);
        }
        let weak = Rc::downgrade(self);
        let task = task.clone();
        let add = button("+", move || {
            if let Some(v) = weak.upgrade() {
                v.add_subtask(&task);
            }
        });
        add.add_css_class("add-subtask");
        add.set_tooltip_text(Some("Add subtask"));
        add.set_halign(gtk::Align::Start);
        add.set_margin_start(36);
        parent.append(&add);
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
        let selected = self.selected.borrow().clone();
        self.focus_task(&selected);
        if !focus_within(&self.root) {
            self.root.grab_focus();
        }
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
        panel.append(&button("Rename group",move ||{if let Some(v)=weak.upgrade(){v.stack.set_visible_child_name("tasks");let request = json!({"action":"group-rename","source":old,"target":entry.text().as_str(),"groupsRevision":v.state.borrow()["groupsRevision"],"scoped":true,"kind":v.collection.borrow().clone(),"members":v.state.borrow()["tasks"].as_array().into_iter().flatten().filter(|t| s(t,"group")==old && (s(t,"kind")==v.collection.borrow().as_str() || (v.collection.borrow().as_str()=="todo" && s(t,"kind")=="project"))).map(|t|json!({"id":t["id"],"revision":t["revision"]})).collect::<Vec<_>>()});v.request(request);}}));
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
