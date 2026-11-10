//! Compact local-calendar snoozing shared by both application hosts.
use super::*;
use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
fn local_time(day: NaiveDate, hour: u32, minute: u32) -> Result<chrono::DateTime<Local>, String> {
    let date = Local
        .with_ymd_and_hms(day.year(), day.month(), day.day(), hour, minute, 0)
        .single()
        .ok_or("That local time is skipped or ambiguous. Choose another time.")?;
    if date <= Local::now() {
        return Err("Choose a future date and time.".into());
    }
    Ok(date)
}
fn shortcut(label: &str) -> Result<chrono::DateTime<Local>, String> {
    let now = Local::now();
    let today = now.date_naive();
    let (days, hour) = match label {
        "Afternoon" => (0, 14),
        "Evening" => (0, 18),
        "Night" => (0, 21),
        "Tomorrow" => (1, 9),
        "Weekend" => (
            ((5 + 7 - today.weekday().num_days_from_monday()) % 7).max(1) as i64,
            9,
        ),
        _ => ((7 - today.weekday().num_days_from_monday()) as i64, 9),
    };
    let mut day = today + Duration::days(days);
    if days == 0 && local_time(day, hour, 0).is_err() {
        day += Duration::days(1);
    }
    // Saturday's Weekend shortcut means the following Saturday.
    if label == "Weekend" && today.weekday() == chrono::Weekday::Sat {
        day = today + Duration::days(7);
    }
    local_time(day, hour, 0)
}
impl TaskView {
    pub(crate) fn snooze(self: &Rc<Self>, task: Value) {
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
        panel.set_widget_name("snooze-picker");
        panel.add_css_class("snooze-picker");
        panel.set_halign(gtk::Align::Center);
        panel.set_valign(gtk::Align::Start);
        panel.set_size_request(340, -1);
        panel.set_margin_bottom(24);
        let heading = gtk::Label::new(Some("Snooze until"));
        heading.set_xalign(0.);
        heading.add_css_class("snooze-heading");
        panel.append(&heading);
        let title = gtk::Label::new(Some(s(&task, "title")));
        title.set_xalign(0.);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_max_width_chars(38);
        title.add_css_class("muted");
        panel.append(&title);
        let choices = gtk::Grid::new();
        choices.set_column_homogeneous(true);
        choices.set_column_spacing(6);
        choices.set_row_spacing(6);
        for (i, label) in [
            "Afternoon",
            "Evening",
            "Night",
            "Tomorrow",
            "Weekend",
            "Next week",
        ]
        .into_iter()
        .enumerate()
        {
            let weak = Rc::downgrade(self);
            let task = task.clone();
            let option = button(label, move || {
                if let Some(view) = weak.upgrade() {
                    match shortcut(label) {
                        Ok(until) => {
                            view.stack.set_visible_child_name("tasks");
                            view.action("snooze", &task, json!({"until":until.to_rfc3339()}));
                        }
                        Err(message) => view.error(&message),
                    }
                }
            });
            option.add_css_class("snooze-option");
            option.add_css_class(
                [
                    "afternoon",
                    "evening",
                    "night",
                    "tomorrow",
                    "weekend",
                    "weekend",
                ][i],
            );
            choices.attach(&option, i as i32 % 3, i as i32 / 3, 1, 1);
        }
        panel.append(&choices);
        let custom = gtk::Label::new(Some("Choose a date"));
        custom.set_xalign(0.);
        panel.append(&custom);
        let day = Local::now().date_naive() + Duration::days(1);
        let date = gtk::Entry::builder()
            .text(day.to_string())
            .placeholder_text("YYYY-MM-DD or tomorrow")
            .build();
        date.set_widget_name("snooze-date");
        date.set_tooltip_text(Some("Date · YYYY-MM-DD, tomorrow, or a weekday"));
        panel.append(&date);
        let calendar = gtk::Calendar::new();
        calendar.set_widget_name("snooze-calendar");
        calendar.set_hexpand(true);
        calendar.set_show_week_numbers(false);
        calendar.select_day(
            &glib::DateTime::from_local(
                day.year(),
                day.month() as i32,
                day.day() as i32,
                12,
                0,
                0.,
            )
            .unwrap(),
        );
        let entry = date.clone();
        calendar.connect_day_selected(move |calendar| {
            let chosen = calendar.date();
            let text = format!(
                "{:04}-{:02}-{:02}",
                chosen.year(),
                chosen.month(),
                chosen.day_of_month()
            );
            if entry.text().as_str() != text {
                entry.set_text(&text);
            }
        });
        let cal = calendar.downgrade();
        date.connect_activate(move |entry| {
            if let Ok(text) = input::date(entry.text().as_str(), Local::now().date_naive()) {
                if let (Ok(day), Some(calendar)) =
                    (NaiveDate::parse_from_str(&text, "%Y-%m-%d"), cal.upgrade())
                {
                    if let Ok(day) = glib::DateTime::from_local(
                        day.year(),
                        day.month() as i32,
                        day.day() as i32,
                        12,
                        0,
                        0.,
                    ) {
                        calendar.select_day(&day);
                    }
                }
            }
        });
        panel.append(&calendar);
        let time = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let label = gtk::Label::new(Some("Local time"));
        label.set_xalign(0.);
        label.set_hexpand(true);
        time.append(&label);
        let clock = gtk::Entry::builder()
            .text("09:00")
            .placeholder_text("HH:MM")
            .width_chars(5)
            .max_width_chars(5)
            .max_length(5)
            .build();
        clock.set_widget_name("snooze-time");
        clock.set_tooltip_text(Some("Local time in 24-hour format, for example 16:45"));
        gtk::prelude::EditableExt::set_alignment(&clock, 0.5);
        time.append(&clock);
        panel.append(&time);
        let feedback = gtk::Label::new(None);
        feedback.set_widget_name("snooze-feedback");
        feedback.set_wrap(true);
        feedback.set_max_width_chars(40);
        feedback.set_xalign(0.);
        feedback.add_css_class("error");
        feedback.set_visible(false);
        panel.append(&feedback);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.set_halign(gtk::Align::End);
        let weak = Rc::downgrade(self);
        actions.append(&button("Cancel", move || {
            if let Some(view) = weak.upgrade() {
                view.stack.set_visible_child_name("tasks");
                view.render();
                view.focus();
            }
        }));
        let weak = Rc::downgrade(self);
        let save = button("Snooze", move || {
            let Some(view) = weak.upgrade() else { return };
            let chosen = input::date(date.text().as_str(), Local::now().date_naive())
                .and_then(|text| {
                    NaiveDate::parse_from_str(&text, "%Y-%m-%d")
                        .map_err(|_| "Choose a date.".to_owned())
                })
                .and_then(|day| {
                    let text = clock.text();
                    let (hour, minute) = text
                        .split_once(':')
                        .ok_or("Enter a time as HH:MM.".to_owned())?;
                    let hour: u32 = hour
                        .parse()
                        .map_err(|_| "Enter a time as HH:MM.".to_owned())?;
                    let minute: u32 = minute
                        .parse()
                        .map_err(|_| "Enter a time as HH:MM.".to_owned())?;
                    if hour > 23 || minute > 59 {
                        return Err("Use a time between 00:00 and 23:59.".into());
                    }
                    local_time(day, hour, minute)
                });
            match chosen {
                Ok(until) => {
                    view.stack.set_visible_child_name("tasks");
                    view.action("snooze", &task, json!({"until":until.to_rfc3339()}));
                }
                Err(message) => {
                    feedback.set_text(&message);
                    feedback.set_visible(true);
                }
            }
        });
        save.set_widget_name("snooze-save");
        save.add_css_class("primary");
        actions.append(&save);
        panel.append(&actions);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&panel)
            .vexpand(true)
            .build();
        if let Some(old) = self.stack.child_by_name("snooze") {
            self.stack.remove(&old);
        }
        self.stack.add_named(&scroller, Some("snooze"));
        self.stack.set_visible_child_name("snooze");
        calendar.grab_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn named(root: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
        if root.widget_name() == name {
            return Some(root.clone());
        }
        let mut child = root.first_child();
        while let Some(widget) = child {
            if let Some(found) = named(&widget, name) {
                return Some(found);
            }
            child = widget.next_sibling();
        }
        None
    }
    #[test]
    fn snooze_shortcuts_use_local_calendar_mornings() {
        for label in ["Tomorrow", "Weekend", "Next week"] {
            let chosen = shortcut(label).unwrap();
            use chrono::Timelike;
            assert_eq!(chosen.hour(), 9);
            assert_eq!(chosen.minute(), 0);
            assert!(chosen > Local::now());
            if label == "Weekend" {
                assert_eq!(chosen.weekday(), chrono::Weekday::Sat);
            }
            if label == "Next week" {
                assert_eq!(chosen.weekday(), chrono::Weekday::Mon);
            }
        }
        assert!(local_time(Local::now().date_naive() - Duration::days(1), 9, 0).is_err());
    }
    #[test]
    #[ignore = "requires an isolated rendering display"]
    fn snooze_calendar_selects_date_and_rejects_past_times() {
        gtk::init().unwrap();
        let sent = Rc::new(RefCell::new(Vec::<Value>::new()));
        let requests = sent.clone();
        let view = TaskView::new(move |r| requests.borrow_mut().push(r), || {}, None);
        let task =
            json!({"id":"a","kind":"todo","title":"Synthetic snooze","revision":1,"group":"All"});
        view.update(&json!({"tasks":[task]}));
        let window = gtk::Window::builder()
            .default_width(860)
            .default_height(680)
            .child(&view.root)
            .build();
        window.present();
        view.snooze(task.clone());
        let calendar = named(view.root.upcast_ref(), "snooze-calendar")
            .unwrap()
            .downcast::<gtk::Calendar>()
            .unwrap();
        let date = named(view.root.upcast_ref(), "snooze-date")
            .unwrap()
            .downcast::<gtk::Entry>()
            .unwrap();
        let save = named(view.root.upcast_ref(), "snooze-save")
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        date.set_text("2000-01-01");
        save.emit_clicked();
        assert!(sent.borrow().is_empty());
        assert!(named(view.root.upcast_ref(), "snooze-feedback")
            .unwrap()
            .is_visible());
        calendar.select_day(&glib::DateTime::from_local(2099, 2, 17, 12, 0, 0.).unwrap());
        assert_eq!(date.text(), "2099-02-17");
        let clock = named(view.root.upcast_ref(), "snooze-time")
            .unwrap()
            .downcast::<gtk::Entry>()
            .unwrap();
        clock.set_text("16:45");
        view.update(&json!({"tasks":[{ "revision":2, "title":"Changed elsewhere", "id":"a","kind":"todo","group":"All"}]}));
        assert_eq!(view.stack.visible_child_name().as_deref(), Some("snooze"));
        save.emit_clicked();
        let request = sent.borrow()[0].clone();
        let until = chrono::DateTime::parse_from_rfc3339(s(&request, "until"))
            .unwrap()
            .with_timezone(&Local);
        use chrono::Timelike;
        assert_eq!(
            until.date_naive(),
            NaiveDate::from_ymd_opt(2099, 2, 17).unwrap()
        );
        assert_eq!((until.hour(), until.minute()), (16, 45));
        assert_eq!(request["revision"], 2);
        assert_eq!(view.stack.visible_child_name().as_deref(), Some("tasks"));
        window.close();
    }
}
