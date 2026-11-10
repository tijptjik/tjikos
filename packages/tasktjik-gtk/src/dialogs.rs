use super::*;
impl TaskView {
    fn panel(self: &Rc<Self>, name: &str, panel: &gtk::Box) {
        let weak = Rc::downgrade(self);
        panel.append(&button("Cancel", move || {
            if let Some(v) = weak.upgrade() {
                v.stack.set_visible_child_name("tasks");
            }
        }));
        if let Some(old) = self.stack.child_by_name(name) {
            self.stack.remove(&old);
        }
        self.stack.add_named(panel, Some(name));
        self.stack.set_visible_child_name(name);
    }
    pub(crate) fn show_image(self: &Rc<Self>, source: &str) {
        let source = source.to_owned();
        self.media(
            move || media::decode(source),
            |view, texture| {
                let picture = gtk::Picture::for_paintable(&texture);
                picture.set_can_shrink(true);
                picture.set_vexpand(true);
                let panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
                panel.append(&picture);
                view.panel("image", &panel);
            },
        );
    }
    pub(crate) fn paste_image(self: &Rc<Self>, task: Value) {
        let weak = Rc::downgrade(self);
        let epoch = self.media_epoch.get();
        self.root
            .clipboard()
            .read_texture_async(None::<&gtk::gio::Cancellable>, move |result| {
                let Some(view) = weak.upgrade() else { return };
                if view.media_epoch.get() != epoch || view.pending.get() {
                    return;
                }
                match result {
                    Ok(Some(texture)) if texture.width() <= 8192 && texture.height() <= 8192 => {
                        view.media(
                            move || media::encode(&texture.save_to_png_bytes()),
                            move |view, data| {
                                view.action("attach-image", &task, json!({"data":data}));
                            },
                        );
                    }
                    _ => view.error("Copy a PNG, JPEG or WebP image first"),
                }
            });
    }
    pub(crate) fn groups(self: &Rc<Self>) {
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
        panel.append(&gtk::Label::new(Some(
            "Groups · changes apply across collections",
        )));
        let snapshot = self.state.borrow().clone();
        let names: Vec<_> = snapshot["groups"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let source = gtk::DropDown::from_strings(&names);
        panel.append(&source);
        let target = gtk::Entry::builder()
            .placeholder_text("New or destination group name")
            .build();
        panel.append(&target);
        for (label, action) in [
            ("Add", "group-add"),
            ("Rename", "group-rename"),
            ("Merge", "group-merge"),
            ("Remove and move items to All", "group-remove"),
        ] {
            let weak = Rc::downgrade(self);
            let snapshot = snapshot.clone();
            let source = source.clone();
            let target = target.clone();
            panel.append(&button(label, move || {
                let Some(v)=weak.upgrade() else {return};
                let name=snapshot["groups"][source.selected() as usize].as_str().unwrap_or("");
                let members:Vec<_>=snapshot["tasks"].as_array().into_iter().flatten().filter(|t|s(t,"group")==name).map(|t|json!({"id":t["id"],"revision":t["revision"]})).collect();
                v.request(json!({"action":action,"source":name,"target":target.text().as_str(),"groupsRevision":snapshot["groupsRevision"],"members":members,"actor":"user"}));
                v.stack.set_visible_child_name("tasks");
            }));
        }
        self.panel("groups", &panel);
    }
    pub(crate) fn verify(self: &Rc<Self>, task: Value) {
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
        panel.append(&gtk::Label::new(Some("Record verification")));
        let outcome = gtk::DropDown::from_strings(&["Passed", "Failed", "Pending"]);
        let evidence = gtk::Entry::builder()
            .placeholder_text("Evidence: what was checked?")
            .build();
        let artifact = gtk::Entry::builder()
            .placeholder_text("Artifact: link or file reference")
            .build();
        panel.append(&outcome);
        panel.append(&evidence);
        panel.append(&artifact);
        let weak = Rc::downgrade(self);
        panel.append(&button("Record", move || {
            let Some(v) = weak.upgrade() else { return };
            if evidence.text().trim().is_empty() || artifact.text().trim().is_empty() {
                v.error("Provide evidence and an artifact reference"); return;
            }
            v.action("verify", &task, json!({"outcome":(["passed","failed","pending"][outcome.selected() as usize]),"evidence":evidence.text().as_str(),"artifact":artifact.text().as_str()}));
            v.stack.set_visible_child_name("tasks");
        }));
        self.panel("verify", &panel);
    }
    pub(crate) fn snooze(self: &Rc<Self>, task: Value) {
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
        panel.append(&gtk::Label::new(Some("Remind me later")));
        let shortcuts = gtk::FlowBox::new();
        shortcuts.set_selection_mode(gtk::SelectionMode::None);
        for (label, minutes) in [
            ("15 minutes", 15),
            ("1 hour", 60),
            ("3 hours", 180),
            ("Tomorrow", 1440),
            ("Next week", 10080),
        ] {
            let weak = Rc::downgrade(self);
            let task = task.clone();
            shortcuts.insert(&button(label, move || {
                if let Some(v) = weak.upgrade() {
                    v.action("snooze", &task, json!({"until":(chrono::Utc::now()+chrono::Duration::minutes(minutes)).to_rfc3339()}));
                    v.stack.set_visible_child_name("tasks");
                }
            }), -1);
        }
        panel.append(&shortcuts);
        let calendar = gtk::Calendar::new();
        panel.append(&calendar);
        let time = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let hour = gtk::SpinButton::with_range(0., 23., 1.);
        hour.set_value(9.);
        let minute = gtk::SpinButton::with_range(0., 59., 1.);
        time.append(&gtk::Label::new(Some("Local time")));
        time.append(&hour);
        time.append(&minute);
        panel.append(&time);
        let weak = Rc::downgrade(self);
        panel.append(&button("Snooze until selected time", move || {
            use chrono::TimeZone;
            let Some(v) = weak.upgrade() else { return };
            let d = calendar.date();
            let date = chrono::Local
                .with_ymd_and_hms(
                    d.year(),
                    d.month() as u32,
                    d.day_of_month() as u32,
                    hour.value_as_int() as u32,
                    minute.value_as_int() as u32,
                    0,
                )
                .single();
            match date {
                Some(date) if date > chrono::Local::now() => {
                    v.action("snooze", &task, json!({"until":date.to_rfc3339()}));
                    v.stack.set_visible_child_name("tasks");
                }
                _ => v.error("Choose a future, unambiguous local time"),
            }
        }));
        self.panel("snooze", &panel);
    }
}
