use super::*;

pub(crate) struct Editor {
    pub root: gtk::ScrolledWindow,
    pub form: gtk::Box,
    pub saving: Rc<Cell<bool>>,
    pub task: Rc<RefCell<Value>>,
}
fn field(form: &gtk::Box, label: &str, value: &str) -> gtk::Entry {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let heading = gtk::Label::builder().label(label).xalign(0.0).build();
    heading.add_css_class("muted");
    row.append(&heading);
    let entry = gtk::Entry::builder().text(value).hexpand(true).build();
    row.append(&entry);
    form.append(&row);
    entry
}
struct Nodes {
    root: gtk::Box,
    items: RefCell<Vec<Rc<Node>>>,
    choices: bool,
}
struct Node {
    root: gtk::Box,
    original: Value,
    title: gtk::Entry,
    due: gtk::Entry,
    offset: gtk::Entry,
    accept: gtk::CheckButton,
    children: Rc<Nodes>,
    removed: Cell<bool>,
    dependencies: Vec<(String, gtk::CheckButton)>,
}
impl Nodes {
    fn new(value: &Value, choices: bool) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let this = Rc::new(Self {
            root,
            items: RefCell::new(vec![]),
            choices,
        });
        for node in value.as_array().into_iter().flatten() {
            this.add(node.clone());
        }
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let kinds: Vec<_> = if choices {
            vec![
                ("Alternative", "task"),
                ("Exit", "exit"),
                ("Reminder", "reminder"),
            ]
        } else {
            vec![("Task", "task"), ("Group", "group"), ("Choice", "flow")]
        };
        for (label, kind) in kinds {
            let weak = Rc::downgrade(&this);
            actions.append(&button(&format!("+ {label}"),move ||{if let Some(nodes)=weak.upgrade(){nodes.add(json!({"id":uuid::Uuid::new_v4().to_string(),"kind":kind,"title":"","due":"","done":false,"items":[],"offset":"one day before the deadline"}));}}));
        }
        this.root.append(&actions);
        this
    }
    fn add(self: &Rc<Self>, value: Value) {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.add_css_class("card");
        let title = field(
            &root,
            if self.choices {
                "Alternative"
            } else {
                s(&value, "kind")
            },
            s(&value, "title"),
        );
        let due = field(&root, "Deadline (YYYY-MM-DD)", s(&value, "due"));
        due.set_sensitive(!self.choices);
        let offset = field(&root, "Reminder interval", s(&value, "offset"));
        offset.set_visible(s(&value, "kind") == "reminder");
        let accept = gtk::CheckButton::with_label("Accept cancelled dependencies");
        accept.set_active(value["acceptCancelled"] == true);
        root.append(&accept);
        let mut dependencies = Vec::new();
        for previous in self.items.borrow().iter().filter(|n| !n.removed.get()) {
            let id = s(&previous.original, "id").to_owned();
            let choice =
                gtk::CheckButton::with_label(&format!("Requires: {}", previous.title.text()));
            choice.set_active(
                value["requires"]
                    .as_array()
                    .is_some_and(|ids| ids.iter().any(|v| v.as_str() == Some(id.as_str()))),
            );
            root.append(&choice);
            dependencies.push((id, choice));
        }
        let children = Nodes::new(&value["items"], s(&value, "kind") == "flow");
        if !["exit", "reminder"].contains(&s(&value, "kind")) {
            root.append(&children.root);
        }
        let node = Rc::new(Node {
            root: root.clone(),
            original: value,
            title,
            due,
            offset,
            accept,
            children,
            removed: Cell::new(false),
            dependencies,
        });
        for (label, delta) in [("Move up", -1isize), ("Move down", 1)] {
            let owner = Rc::downgrade(self);
            let weak = Rc::downgrade(&node);
            root.append(&button(label, move || {
                let (Some(owner), Some(node)) = (owner.upgrade(), weak.upgrade()) else {
                    return;
                };
                let mut items = owner.items.borrow_mut();
                let Some(index) = items.iter().position(|n| Rc::ptr_eq(n, &node)) else {
                    return;
                };
                let to = index as isize + delta;
                if to < 0 || to >= items.len() as isize {
                    return;
                }
                items.swap(index, to as usize);
                let previous = if to == 0 {
                    None
                } else {
                    Some(&items[to as usize - 1].root)
                };
                owner.root.reorder_child_after(&node.root, previous);
            }));
        }
        let weak = Rc::downgrade(&node);
        root.append(&button("Remove step", move || {
            if let Some(node) = weak.upgrade() {
                node.removed.set(true);
                node.root.set_visible(false);
            }
        }));
        let previous = self.items.borrow().last().map(|n| n.root.clone());
        self.root.insert_child_after(&root, previous.as_ref());
        self.items.borrow_mut().push(node);
    }
    fn value(&self) -> Value {
        Value::Array(
            self.items
                .borrow()
                .iter()
                .filter(|n| !n.removed.get())
                .map(|n| {
                    let mut value = n.original.clone();
                    value["title"] = json!(n.title.text().as_str());
                    value["due"] = json!(n.due.text().as_str());
                    value["offset"] = json!(n.offset.text().as_str());
                    value["acceptCancelled"] = json!(n.accept.is_active());
                    value["items"] = n.children.value();
                    value["requires"] = json!(n
                        .dependencies
                        .iter()
                        .filter(|(_, b)| b.is_active())
                        .map(|(id, _)| id)
                        .collect::<Vec<_>>());
                    value
                })
                .collect(),
        )
    }
}
impl TaskView {
    pub fn edit(self: &Rc<Self>, task: Value) {
        if self.pending.get() {
            return;
        }
        self.close_editor();
        let form = gtk::Box::new(gtk::Orientation::Vertical, 10);
        form.add_css_class("pane");
        let tracked_task = Rc::new(RefCell::new(task.clone()));
        let title = field(&form, "Title", s(&task, "title"));
        let group = field(
            &form,
            "Group",
            if s(&task, "group").is_empty() {
                "All"
            } else {
                s(&task, "group")
            },
        );
        let tags = field(
            &form,
            "Tags (comma separated)",
            &task["tags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        );
        let due = field(
            &form,
            "Due date (YYYY-MM-DD, today or tomorrow)",
            s(&task, "due"),
        );
        let reference = field(
            &form,
            if s(&task, "kind") == "reading" {
                "Web address"
            } else {
                "Reference or note"
            },
            s(&task, "reference"),
        );
        let plan = field(
            &form,
            "Plan (vault-relative Markdown path)",
            s(&task, "plan"),
        );
        let assignee = field(&form, "Assignee", s(&task, "assignee"));
        let acceptance = field(
            &form,
            "Acceptance criteria (separate with semicolons)",
            &task["acceptance"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("; "),
        );
        let project = gtk::DropDown::from_strings(&["No project"]);
        let mut projects = vec![json!({"id":"","title":"No project"})];
        projects.extend(
            self.state.borrow()["tasks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|t| s(t, "kind") == "project" && t["id"] != task["id"])
                .cloned(),
        );
        project.set_model(Some(&gtk::StringList::new(
            &projects.iter().map(|t| s(t, "title")).collect::<Vec<_>>(),
        )));
        project.set_selected(
            projects
                .iter()
                .position(|p| p["id"] == task["project"])
                .unwrap_or(0) as u32,
        );
        form.append(&gtk::Label::new(Some("Project")));
        form.append(&project);
        let only_project = gtk::CheckButton::with_label("Show only inside its project");
        only_project.set_active(task["onlyInProject"] == true);
        form.append(&only_project);
        let repeat = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let every = gtk::SpinButton::with_range(1.0, 10000.0, 1.0);
        every.set_value(task["repeat"]["value"].as_f64().unwrap_or(1.));
        let units = ["Never", "days", "weeks", "months", "years"];
        let unit = gtk::DropDown::from_strings(&units);
        unit.set_selected(
            units
                .iter()
                .position(|v| *v == s(&task["repeat"], "unit"))
                .unwrap_or(0) as u32,
        );
        repeat.append(&gtk::Label::new(Some("Repeat every")));
        repeat.append(&every);
        repeat.append(&unit);
        form.append(&repeat);
        let notify = gtk::CheckButton::with_label("Notify before due");
        notify.set_active(task["notifyBeforeDue"] != false);
        form.append(&notify);
        let lead = gtk::SpinButton::with_range(0., 10000., 1.);
        lead.set_value(task["notifyBeforeDueLeadValue"].as_f64().unwrap_or(1.));
        let lead_units = ["minutes", "hours", "days", "weeks"];
        let lead_unit = gtk::DropDown::from_strings(&lead_units);
        lead_unit.set_selected(
            lead_units
                .iter()
                .position(|v| *v == s(&task, "notifyBeforeDueLeadUnit"))
                .unwrap_or(2) as u32,
        );
        let reminder = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        reminder.append(&lead);
        reminder.append(&lead_unit);
        form.append(&reminder);
        let sequential = gtk::CheckButton::with_label("Complete workflow steps in order");
        sequential.set_active(task["workflow"] == true);
        form.append(&sequential);
        let nodes = Nodes::new(&task["subtasks"], false);
        let templates: Vec<_> = self.state.borrow()["tasks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| s(t, "kind") == "template")
            .cloned()
            .collect();
        if !templates.is_empty() && s(&task, "kind") != "template" {
            let mut names = vec!["Use a workflow template…"];
            names.extend(templates.iter().map(|t| s(t, "title")));
            let picker = gtk::DropDown::from_strings(&names);
            let target = nodes.clone();
            let sequential = sequential.clone();
            picker.connect_selected_notify(move |picker| {
                if picker.selected() == 0 {
                    return;
                }
                let template = &templates[picker.selected() as usize - 1];
                for node in target.items.borrow().iter() {
                    target.root.remove(&node.root);
                }
                target.items.borrow_mut().clear();
                let mut value = template["subtasks"].clone();
                fn renew(value: &mut Value, ids: &mut std::collections::HashMap<String, String>) {
                    for node in value.as_array_mut().into_iter().flatten() {
                        let id = uuid::Uuid::new_v4().to_string();
                        ids.insert(s(node, "id").to_owned(), id.clone());
                        node["id"] = json!(id);
                        node["done"] = json!(false);
                        renew(&mut node["items"], ids);
                    }
                }
                fn dependencies(
                    value: &mut Value,
                    ids: &std::collections::HashMap<String, String>,
                ) {
                    for node in value.as_array_mut().into_iter().flatten() {
                        if let Some(requires) = node["requires"].as_array_mut() {
                            for id in requires {
                                if let Some(new) = id.as_str().and_then(|id| ids.get(id)) {
                                    *id = json!(new);
                                }
                            }
                        }
                        dependencies(&mut node["items"], ids);
                    }
                }
                let mut ids = std::collections::HashMap::new();
                renew(&mut value, &mut ids);
                dependencies(&mut value, &ids);
                for node in value.as_array().into_iter().flatten() {
                    target.add(node.clone());
                }
                sequential.set_active(template["workflow"] == true);
            });
            form.append(&picker);
        }
        form.append(&nodes.root);
        if !s(&task, "id").is_empty() {
            let weak = Rc::downgrade(self);
            let tracked = tracked_task.clone();
            form.append(&button("Attach image…", move || {
                let Some(view) = weak.upgrade() else { return };
                let dialog = gtk::FileDialog::new();
                let weak = Rc::downgrade(&view);
                let tracked = tracked.clone();
                let epoch = view.media_epoch.get();
                let parent = view
                    .root
                    .root()
                    .and_then(|w| w.downcast::<gtk::Window>().ok());
                dialog.open(
                    parent.as_ref(),
                    None::<&gtk::gio::Cancellable>,
                    move |result| {
                        let Some(view) = weak.upgrade() else { return };
                        if view.media_epoch.get() != epoch || !view.is_editing() {
                            return;
                        }
                        let Ok(file) = result else { return };
                        let Some(path) = file.path() else { return };
                        let task = tracked.borrow().clone();
                        view.media(
                            move || media::read_file(path),
                            move |view, data| {
                                let current = view.editor.borrow().as_ref().is_some_and(|editor| {
                                    Rc::ptr_eq(&editor.task, &tracked)
                                        && !editor.saving.get()
                                        && *tracked.borrow() == task
                                });
                                if current {
                                    view.action("attach-image", &task, json!({"data":data}));
                                }
                            },
                        );
                    },
                );
            }));
        }
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let save = gtk::Button::with_label("Save");
        buttons.append(&save);
        let keys = gtk::EventControllerKey::new();
        let save_key = save.clone();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, mods| {
            if key == gdk::Key::Return && mods.contains(gdk::ModifierType::CONTROL_MASK) {
                save_key.emit_clicked();
            } else if key == gdk::Key::Escape {
                if let Some(v) = weak.upgrade() {
                    if !v.pending.get() {
                        let draft = s(&v.state.borrow(), "draft_id").to_owned();
                        v.close_editor();
                        if !draft.is_empty() {
                            v.request(json!({"action":"reading-cancel","draft_id":draft}));
                        } else {
                            v.render();
                        }
                    }
                }
            } else {
                return glib::Propagation::Proceed;
            }
            glib::Propagation::Stop
        });
        form.add_controller(keys);
        let weak = Rc::downgrade(self);
        buttons.append(&button("Cancel", move || {
            if let Some(v) = weak.upgrade() {
                if v.pending.get() {
                    return;
                }
                let draft = s(&v.state.borrow(), "draft_id").to_owned();
                v.close_editor();
                if !draft.is_empty() {
                    v.request(json!({"action":"reading-cancel","draft_id":draft}));
                } else {
                    v.render();
                }
            }
        }));
        form.append(&buttons);
        let root = gtk::ScrolledWindow::builder()
            .child(&form)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        self.stack.add_named(&root, Some("editor"));
        self.stack.set_visible_child_name("editor");
        let saving = Rc::new(Cell::new(false));
        let weak = Rc::downgrade(self);
        let saving_clone = saving.clone();
        let form_clone = form.clone();
        let request_id = uuid::Uuid::new_v4().to_string();
        title.grab_focus();
        let tracked = tracked_task.clone();
        save.connect_clicked(move |_|{
            let Some(view)=weak.upgrade()else{return};if view.pending.get(){return;}
            if title.text().trim().is_empty(){view.error("Title is required");title.grab_focus();return;}
            let date=match due.text().trim(){"today"=>chrono::Local::now().date_naive().to_string(),"tomorrow"=>(chrono::Local::now().date_naive()+chrono::Duration::days(1)).to_string(),other=>other.to_owned()};
            if !date.is_empty() && chrono::NaiveDate::parse_from_str(&date,"%Y-%m-%d").is_err(){view.error("Use a date such as 2026-11-09");return;}
            let mut recurrence=task["repeat"].clone();if unit.selected()==0{recurrence=json!({});}else{if !recurrence.is_object(){recurrence=json!({});}recurrence["unit"]=json!(units[unit.selected()as usize]);recurrence["value"]=json!(every.value_as_int());}
            let fields=json!({"title":title.text().trim(),"plan":plan.text().as_str(),"assignee":assignee.text().as_str(),"acceptance":acceptance.text().split(';').map(str::trim).filter(|v|!v.is_empty()).collect::<Vec<_>>(),"kind":if s(&task,"kind").is_empty(){"todo"}else{s(&task,"kind")},"group":group.text().trim(),"tags":tags.text().split(',').map(str::trim).filter(|v|!v.is_empty()).collect::<Vec<_>>(),"due":date,"reference":reference.text().as_str(),"project":projects[project.selected()as usize]["id"],"onlyInProject":only_project.is_active(),"repeat":recurrence,"notifyBeforeDue":notify.is_active(),"notifyBeforeDueLeadValue":lead.value_as_int(),"notifyBeforeDueLeadUnit":lead_units[lead_unit.selected()as usize],"subtasks":nodes.value(),"workflow":sequential.is_active()});
            let draft=s(&view.state.borrow(),"draft_id").to_owned();saving_clone.set(true);form_clone.set_sensitive(false);
            view.request(json!({"action":if !draft.is_empty(){"reading-save"}else if s(&task,"id").is_empty(){"create"}else{"edit"},"id":task["id"],"revision":tracked.borrow()["revision"],"draft_id":draft,"request_id":request_id,"fields":fields}));
        });
        *self.editor.borrow_mut() = Some(Editor {
            root,
            form,
            saving,
            task: tracked_task,
        });
    }
}
