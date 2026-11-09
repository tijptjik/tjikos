//! Notjik remains the notification authority. This host only reads history and
//! submits keyed actions; it never owns the notification bus or history files.
use crate::{
    panel_view::{control_symbol, Content},
    util::{arr, s},
    Event,
};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
    time::Duration,
};
use tjik_gtk::gtk::{self, glib, prelude::*};

fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    l.add_css_class(class);
    l
}
fn visible_notice(n: &Value) -> bool {
    !["app", "desktopEntry"].iter().any(|key| {
        let value = s(&n[*key]).trim().to_lowercase();
        matches!(
            value.trim_end_matches(".desktop"),
            "signal" | "signal desktop" | "signal-desktop" | "org.signal.signal"
        )
    })
}
fn group_name(n: &Value) -> &str {
    ["app", "desktopEntry"]
        .iter()
        .find_map(|k| n[*k].as_str().filter(|v| !v.trim().is_empty()))
        .unwrap_or("Notification")
}
fn matches(n: &Value, query: &str) -> bool {
    let text = [
        "device",
        "run",
        "title",
        "body",
        "app",
        "desktopEntry",
        "group",
    ]
    .map(|k| s(&n[k]))
    .join("\n")
    .to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|q| text.contains(q))
}
fn group_key(n: &Value) -> String {
    json!([
        n["desktopEntry"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| s(&n["app"])),
        n["group"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| s(&n["senderId"]))
    ])
    .to_string()
}
fn fixture() -> Value {
    json!({"ok":true,"locked":false,"paused":false,"items":[
        {"historyKey":"demo/1","app":"Calendar","title":"Weekend plans","body":"Meet at the Central pier","received":1794222000000i64,"archived":true,"actions":[]},
        {"historyKey":"demo/2","app":"Calendar","title":"Design review","body":"Bring the updated panel designs","received":1794221000000i64,"archived":false,"actions":[{"label":"Open","key":"default"}]},
        {"historyKey":"demo/signal","app":"Signal","title":"Private Signal message","body":"Must not appear","received":1794220000000i64,"archived":true,"actions":[]},
        {"historyKey":"demo/3","app":"Mail","title":"Booking confirmed","body":"Your reservation is confirmed.","received":1794220000000i64,"archived":true,"actions":[]}
    ]})
}
struct Session {
    stop: tokio::sync::watch::Sender<bool>,
    commands: tokio::sync::mpsc::Sender<Vec<String>>,
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}
fn start(
    demo: bool,
) -> (
    Session,
    async_channel::Receiver<(Result<Value, String>, bool)>,
) {
    let (stop, mut cancelled) = tokio::sync::watch::channel(false);
    let (commands, mut requests) = tokio::sync::mpsc::channel::<Vec<String>>(4);
    let (send, receive) = async_channel::bounded(2);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let work = async {
                let mut tick = tokio::time::interval(Duration::from_secs(1));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut fake = fixture();
                loop {
                    let mut complete = false;
                    let mut failure = None;
                    tokio::select! {
                        _ = tick.tick() => {},
                        request = requests.recv() => {
                            let Some(args) = request else { break; };
                            complete = true;
                            if demo {
                                if args[0] == "pause" { fake["paused"] = true.into(); }
                                else if args[0] == "resume" { fake["paused"] = false.into(); }
                                else if args.get(2).map(String::as_str) == Some("dismiss") {
                                    if let Some(n) = fake["items"].as_array_mut().unwrap().iter_mut().find(|n| n["historyKey"] == args[1]) { n["archived"] = true.into(); }
                                }
                            } else {
                                let mut command = vec!["notjikctl".into()]; command.extend(args);
                                if crate::util::command(&command, 3., 1048576, &[], None).await.is_err() { failure = Some("Notification action failed or is no longer available".to_owned()); }
                            }
                        }
                    }
                    let result = if let Some(error) = failure { Err(error) }
                        else if demo { Ok(fake.clone()) }
                        else { crate::util::command(&["notjikctl".into(),"history-data".into()], 3., 16*1024*1024, &[], None).await
                            .and_then(|s| Ok(serde_json::from_str::<Value>(&s)?))
                            .and_then(|v| { anyhow::ensure!(v["ok"] == true, "unavailable"); Ok(v) })
                            .map_err(|_| "Notification history unavailable".to_owned()) };
                    if send.send((result, complete)).await.is_err() { break; }
                }
            };
            tokio::select! { _ = cancelled.changed() => {}, _ = work => {} }
        });
    });
    (Session { stop, commands }, receive)
}
pub struct Card {
    body: gtk::Box,
    list: gtk::Box,
    search: gtk::SearchEntry,
    count: gtk::Label,
    message: gtk::Label,
    pause: gtk::Button,
    data: RefCell<Value>,
    expanded: RefCell<HashSet<String>>,
    search_closed: RefCell<HashSet<String>>,
    selected: RefCell<String>,
    session: RefCell<Option<Session>>,
    epoch: Cell<u64>,
    busy: Cell<bool>,
    media: RefCell<Vec<gtk::MediaFile>>,
    cards: RefCell<Vec<notjik_core::gtk::Card>>,
    card_css: RefCell<Option<gtk::CssProvider>>,
    card_style: RefCell<Value>,
    demo: bool,
    send: async_channel::Sender<Event>,
}
impl Card {
    pub fn new(demo: bool, send: async_channel::Sender<Event>) -> Rc<Self> {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        body.set_focusable(true);
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let count = label("Loading notifications…", "panel-muted");
        count.set_hexpand(true);
        let pause = gtk::Button::new();
        pause.add_css_class("panel-control");
        pause.set_child(Some(&control_symbol("notifications_paused")));
        pause.update_property(&[gtk::accessible::Property::Label(
            "Pause or resume notifications",
        )]);
        toolbar.append(&count);
        toolbar.append(&pause);
        body.append(&toolbar);
        let search = gtk::SearchEntry::new();
        search.add_css_class("history-search");
        search.set_visible(false);
        search.set_placeholder_text(Some("Filter messages…"));
        search.update_property(&[gtk::accessible::Property::Label(
            "Filter notification history",
        )]);
        body.append(&search);
        let message = label("", "panel-muted");
        message.set_visible(false);
        message.set_wrap(true);
        body.append(&message);
        let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
        body.append(&list);
        let c = Rc::new(Self {
            body,
            list,
            search,
            count,
            message,
            pause,
            data: RefCell::new(Value::Null),
            expanded: RefCell::new(HashSet::new()),
            search_closed: RefCell::new(HashSet::new()),
            selected: RefCell::new(String::new()),
            session: RefCell::new(None),
            epoch: Cell::new(0),
            busy: Cell::new(false),
            media: RefCell::new(Vec::new()),
            cards: RefCell::new(Vec::new()),
            card_css: RefCell::new(None),
            card_style: RefCell::new(Value::Null),
            demo,
            send,
        });
        let weak = Rc::downgrade(&c);
        c.body.connect_map(move |_| {
            if let Some(c) = weak.upgrade() {
                c.begin();
            }
        });
        let weak = Rc::downgrade(&c);
        c.body.connect_unmap(move |_| {
            if let Some(c) = weak.upgrade() {
                c.stop();
            }
        });
        let weak = Rc::downgrade(&c);
        c.search.connect_stop_search(move |_| {
            if let Some(c) = weak.upgrade() {
                c.close_search();
            }
        });
        let weak = Rc::downgrade(&c);
        c.search.connect_search_changed(move |_| {
            if let Some(c) = weak.upgrade() {
                c.search_closed.borrow_mut().clear();
                c.render();
            }
        });
        let weak = Rc::downgrade(&c);
        c.pause.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                let op = if c.data.borrow()["paused"] == true {
                    "resume"
                } else {
                    "pause"
                };
                c.run(vec![op.into()]);
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&c);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(c) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            use gtk::gdk::Key;
            if key == Key::Escape && c.search.is_visible() {
                c.close_search();
                return glib::Propagation::Stop;
            }
            if c.search.is_visible()
                && c.search
                    .state_flags()
                    .contains(gtk::StateFlags::FOCUS_WITHIN)
            {
                return glib::Propagation::Proceed;
            }
            if key == Key::slash {
                c.open_search();
                return glib::Propagation::Stop;
            }
            if key == Key::Down || key == Key::Up {
                let data = c.data.borrow().clone();
                let query = c.search.text();
                let items: Vec<_> = arr(&data["items"])
                    .iter()
                    .filter(|n| visible_notice(n) && matches(n, &query))
                    .collect();
                if !items.is_empty() {
                    let current = items
                        .iter()
                        .position(|n| n["historyKey"] == *c.selected.borrow());
                    let index = current
                        .map(|i| {
                            if key == Key::Down {
                                (i + 1) % items.len()
                            } else {
                                (i + items.len() - 1) % items.len()
                            }
                        })
                        .unwrap_or(0);
                    *c.selected.borrow_mut() = s(&items[index]["historyKey"]).into();
                    c.expanded.borrow_mut().insert(group_key(items[index]));
                    c.render();
                }
                return glib::Propagation::Stop;
            }
            if key == Key::Return || key == Key::Delete {
                let selected = c.selected.borrow().clone();
                c.action(
                    &selected,
                    if key == Key::Delete {
                        "dismiss"
                    } else {
                        "invoke"
                    },
                    0,
                );
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        c.body.add_controller(keys);
        let shortcuts = gtk::ShortcutController::new();
        shortcuts.set_scope(gtk::ShortcutScope::Managed);
        let weak = Rc::downgrade(&c);
        shortcuts.add_shortcut(gtk::Shortcut::new(
            Some(gtk::KeyvalTrigger::new(
                gtk::gdk::Key::slash,
                gtk::gdk::ModifierType::empty(),
            )),
            Some(gtk::CallbackAction::new(move |_, _| {
                if let Some(c) = weak.upgrade() {
                    if c.search
                        .state_flags()
                        .contains(gtk::StateFlags::FOCUS_WITHIN)
                    {
                        return glib::Propagation::Proceed;
                    }
                    c.open_search();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            })),
        ));
        c.body.add_controller(shortcuts);
        c
    }
    pub fn content(&self) -> Content {
        Content {
            title: "Notifications".into(),
            body: self.body.clone().upcast(),
            footer: None,
            preferred_width: 480.,
            needs_keyboard: true,
        }
    }
    fn open_search(&self) {
        self.search.set_visible(true);
        self.search.grab_focus();
        self.resize();
    }
    fn close_search(&self) {
        self.search.set_text("");
        self.search.set_visible(false);
        self.body.grab_focus();
        self.resize();
    }
    fn stop(&self) {
        self.cards.borrow_mut().clear();
        self.search.set_visible(false);
        for media in self.media.borrow_mut().drain(..) {
            media.pause();
        }
        self.epoch.set(self.epoch.get() + 1);
        self.session.borrow_mut().take();
        *self.data.borrow_mut() = Value::Null;
        self.expanded.borrow_mut().clear();
        self.selected.borrow_mut().clear();
        self.busy.set(false);
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        self.search.set_text("");
        self.message.set_text("");
        self.message.set_visible(false);
        self.count.set_text("Loading notifications…");
    }
    fn begin(self: &Rc<Self>) {
        if self.session.borrow().is_some() {
            return;
        }
        let (session, updates) = start(self.demo);
        *self.session.borrow_mut() = Some(session);
        let weak = Rc::downgrade(self);
        let epoch = self.epoch.get();
        glib::spawn_future_local(async move {
            while let Ok((result, complete)) = updates.recv().await {
                let Some(c) = weak.upgrade() else {
                    break;
                };
                if epoch != c.epoch.get() {
                    break;
                }
                if complete {
                    c.busy.set(false);
                }
                let data = match result {
                    Ok(v) => {
                        c.message.set_text("");
                        c.message.set_visible(false);
                        v
                    }
                    Err(e) => {
                        c.message.set_text(&e);
                        c.message.set_visible(true);
                        Value::Null
                    }
                };
                if data != *c.data.borrow() || complete {
                    *c.data.borrow_mut() = data;
                    c.render();
                }
            }
        });
    }
    fn run(self: &Rc<Self>, args: Vec<String>) {
        if self.busy.get() || self.data.borrow()["locked"] != false {
            return;
        }
        if self
            .session
            .borrow()
            .as_ref()
            .is_some_and(|s| s.commands.try_send(args).is_ok())
        {
            self.busy.set(true);
            self.render();
        } else {
            self.message.set_text("Notification service unavailable");
        }
    }
    fn action(self: &Rc<Self>, key: &str, action: &str, index: usize) {
        if !arr(&self.data.borrow()["items"])
            .iter()
            .any(|n| n["historyKey"] == key && n["archived"] != true)
        {
            return;
        }
        self.run(vec![
            "history-action".into(),
            key.into(),
            action.into(),
            "--index".into(),
            index.to_string(),
        ]);
    }
    fn render(self: &Rc<Self>) {
        self.cards.borrow_mut().clear();
        for media in self.media.borrow_mut().drain(..) {
            media.pause();
        }
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let data = self.data.borrow().clone();
        let unlocked = data["locked"] == false;
        self.search.set_sensitive(unlocked);
        self.pause.set_sensitive(unlocked && !self.busy.get());
        self.pause
            .set_child(Some(&control_symbol(if data["paused"] == true {
                "notifications_off"
            } else {
                "notifications_active"
            })));
        if !unlocked {
            self.search.set_visible(false);
            self.selected.borrow_mut().clear();
            self.expanded.borrow_mut().clear();
            self.count
                .set_text("Notification history is locked or unavailable");
            self.resize();
            return;
        }
        let query = self.search.text().to_string();
        let items: Vec<_> = arr(&data["items"])
            .iter()
            .filter(|n| visible_notice(n))
            .cloned()
            .collect();
        let filtered: Vec<_> = items
            .iter()
            .filter(|n| visible_notice(n) && matches(n, &query))
            .collect();
        self.count.set_text(&format!(
            "{} / {} notifications{}",
            filtered.len(),
            items.len(),
            if data["paused"] == true {
                " · Paused"
            } else {
                ""
            }
        ));
        let mut groups: Vec<(String, Vec<&Value>)> = vec![];
        for n in filtered {
            let key = if data["config"]["grouping"]["enabled"] == false {
                s(&n["historyKey"]).into()
            } else {
                group_key(n)
            };
            if let Some((_, notices)) = groups.iter_mut().find(|(k, _)| *k == key) {
                notices.push(n);
            } else {
                groups.push((key, vec![n]));
            }
        }
        if groups.is_empty() {
            self.list.append(&label(
                if query.is_empty() {
                    "No saved notifications yet"
                } else {
                    "No matching notifications"
                },
                "panel-muted",
            ));
        }
        for (key, notices) in groups {
            let open = if query.trim().is_empty() {
                self.expanded.borrow().contains(&key)
            } else {
                !self.search_closed.borrow().contains(&key)
            };
            let head = gtk::Button::new();
            head.add_css_class("history-group");
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            line.append(&control_symbol(if open {
                "expand_more"
            } else {
                "chevron_right"
            }));
            let group = s(&notices[0]["group"]);
            let name = label(
                &format!(
                    "{}{}{}",
                    group_name(notices[0]),
                    if group.is_empty() { "" } else { " · " },
                    group
                ),
                "network-label",
            );
            name.set_hexpand(true);
            line.append(&name);
            line.append(&label(&notices.len().to_string(), "network-label"));
            head.set_child(Some(&line));
            let weak = Rc::downgrade(self);
            let key2 = key.clone();
            head.connect_clicked(move |_| {
                if let Some(c) = weak.upgrade() {
                    if c.search.text().trim().is_empty() {
                        if !c.expanded.borrow_mut().remove(&key2) {
                            c.expanded.borrow_mut().insert(key2.clone());
                        }
                    } else if !c.search_closed.borrow_mut().remove(&key2) {
                        c.search_closed.borrow_mut().insert(key2.clone());
                    }
                    c.render();
                    c.body.grab_focus();
                }
            });
            self.list.append(&head);
            if !open {
                continue;
            }
            for n in notices {
                let key = s(&n["historyKey"]).to_owned();
                let selected = *self.selected.borrow() == key;
                let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
                let button = gtk::Button::new();
                button.add_css_class("history-notice");
                let summary = gtk::Box::new(gtk::Orientation::Vertical, 4);
                let title = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let text = label(s(&n["title"]), "history-title");
                text.set_hexpand(true);
                title.append(&text);
                let date = n["received"]
                    .as_i64()
                    .and_then(|ms| chrono::DateTime::from_timestamp_millis(ms))
                    .map(|d| {
                        let date = d.with_timezone(&chrono::Local);
                        date.format(if date.date_naive() == chrono::Local::now().date_naive() {
                            "%H:%M"
                        } else {
                            "%d %b %H:%M"
                        })
                        .to_string()
                    })
                    .unwrap_or_default();
                title.append(&label(&date, "panel-muted"));
                summary.append(&title);
                let text = label(s(&n["body"]), "panel-muted");
                if selected {
                    text.set_ellipsize(gtk::pango::EllipsizeMode::None);
                    text.set_wrap(true);
                    text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                }
                summary.append(&text);
                button.set_child(Some(&summary));
                let weak = Rc::downgrade(self);
                let key2 = key.clone();
                button.connect_clicked(move |_| {
                    if let Some(c) = weak.upgrade() {
                        *c.selected.borrow_mut() = if *c.selected.borrow() == key2 {
                            String::new()
                        } else {
                            key2.clone()
                        };
                        c.render();
                        c.body.grab_focus();
                    }
                });
                if !selected {
                    row.append(&button);
                }
                if selected {
                    let mut style = data["config"]["style"].clone();
                    if !style.is_object() {
                        style = json!({"background":"#1f1d2e","surface":"#26233a","text":"#e5e5e5","muted":"#a5a0ba","accent":"#ff53a6","font":"tijpset"});
                    }
                    for (k, v) in [("fontSize", 14), ("padding", 12), ("radius", 8)] {
                        if style[k].is_null() {
                            style[k] = v.into();
                        }
                    }
                    if *self.card_style.borrow() != style {
                        if let Some(old) = self.card_css.borrow_mut().take() {
                            gtk::style_context_remove_provider_for_display(
                                &gtk::gdk::Display::default().unwrap(),
                                &old,
                            );
                        }
                        *self.card_css.borrow_mut() =
                            Some(notjik_core::gtk::primitives::apply_theme(&style));
                        *self.card_style.borrow_mut() = style;
                    }
                    let host = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    host.add_css_class("notjik");
                    let card = notjik_core::gtk::Card::new(
                        notjik_core::gtk::model::Notice(n.clone()),
                        notjik_core::gtk::model::ViewState {
                            locked: false,
                            history: true,
                            ..Default::default()
                        },
                        &crate::util::root().join("lib/notification-cards"),
                        chrono::Local::now().timestamp_millis() as f64,
                        |_| {},
                    );
                    fn no_tooltips(w: &gtk::Widget) {
                        w.set_has_tooltip(false);
                        let mut child = w.first_child();
                        while let Some(c) = child {
                            no_tooltips(&c);
                            child = c.next_sibling();
                        }
                    }
                    no_tooltips(card.widget.upcast_ref());
                    host.append(&card.widget);
                    row.append(&host);
                    self.cards.borrow_mut().push(card);
                    let source = s(&n["data"]["source"]);
                    let local = source.strip_prefix("file://").unwrap_or(source);
                    if n["data"]["kind"] == "voice"
                        && local.starts_with('/')
                        && std::path::Path::new(local).is_file()
                    {
                        let media = gtk::MediaFile::for_filename(local);
                        let play = gtk::Button::new();
                        play.add_css_class("panel-control");
                        play.set_child(Some(&control_symbol("play_arrow")));
                        play.update_property(&[gtk::accessible::Property::Label(
                            "Play or pause voice message",
                        )]);
                        let stream = media.clone();
                        play.connect_clicked(move |_| {
                            if stream.is_playing() {
                                stream.pause();
                            } else {
                                stream.play();
                            }
                        });
                        let weak = play.downgrade();
                        media.connect_playing_notify(move |m| {
                            if let Some(b) = weak.upgrade() {
                                b.set_child(Some(&control_symbol(if m.is_playing() {
                                    "pause"
                                } else {
                                    "play_arrow"
                                })));
                            }
                        });
                        let error = label("", "panel-muted");
                        error.set_visible(false);
                        let weak_error = error.downgrade();
                        media.connect_error_notify(move |m| {
                            if let Some(l) = weak_error.upgrade() {
                                l.set_visible(m.error().is_some());
                                l.set_text("Unable to play this recording");
                            }
                        });
                        row.append(&error);
                        row.append(&play);
                        self.media.borrow_mut().push(media);
                    }
                }
                self.list.append(&row);
            }
        }
        self.resize();
    }
    fn resize(&self) {
        let _ = self.send.try_send(Event::PanelCommand("resize".into()));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matching_and_identity() {
        let data = fixture();
        let n = &data["items"][0];
        assert!(!visible_notice(
            &json!({"desktopEntry":"org.signal.Signal.desktop"})
        ));
        assert!(!visible_notice(&json!({"app":"Signal"})));
        assert!(visible_notice(&json!({"app":"Signal processing"})));
        assert!(matches(n, "calendar pier"));
        assert!(!matches(n, "calendar missing"));
        assert_ne!(group_key(n), group_key(&data["items"][2]));
    }
}

#[cfg(test)]
pub fn exercise_fixture(card: &Rc<Card>) {
    assert_eq!(card.data.borrow()["locked"], false);
    assert!(!card.search.is_visible());
    card.open_search();
    assert!(card.search.is_visible());
    assert!(card.search.measure(gtk::Orientation::Vertical, 388).1 <= 36);
    card.search.emit_by_name::<()>("stop-search", &[]);
    assert!(!card.search.is_visible());
    assert!(card.count.text().contains("3 / 3"));
    let group = card
        .list
        .first_child()
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap();
    group.emit_clicked();
    assert!(card.list.first_child().unwrap().next_sibling().is_some());
    let row = card.list.first_child().unwrap().next_sibling().unwrap();
    row.first_child()
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap()
        .emit_clicked();
    assert_eq!(*card.selected.borrow(), "demo/1");
    assert_eq!(card.cards.borrow().len(), 1);
    assert!(card.cards.borrow()[0].widget.has_css_class("card-frame"));
    assert!(card.cards.borrow()[0].state().history);
    let window = card.body.root().unwrap().downcast::<gtk::Window>().unwrap();
    let child = window.child().unwrap();
    let height = child.measure(gtk::Orientation::Vertical, 420).1;
    child.allocate(420, height, -1, None);
    let snapshot = gtk::Snapshot::new();
    window.snapshot_child(&child, &snapshot);
    let node = snapshot.to_node().unwrap();
    window
        .renderer()
        .unwrap()
        .render_texture(&node, None)
        .save_to_png("/tmp/bartjik-history-detail.png")
        .unwrap();

    card.search.set_text("pier");
    card.render();
    assert!(card.count.text().contains("1 / 3"));
    card.search.set_text("");
    *card.data.borrow_mut() = json!({"ok":true,"locked":true,"items":[{"title":"secret"}]});
    card.render();
    assert!(card.list.first_child().is_none());
    assert!(!card.search.is_sensitive());
    card.stop();
    assert!(card.session.borrow().is_none());
    assert!(card.data.borrow().is_null());
}

#[cfg(test)]
mod worker_tests {
    use super::*;
    #[tokio::test]
    async fn private_pause_dismiss_and_shutdown() {
        let (session, updates) = start(true);
        assert_eq!(
            updates.recv().await.unwrap().0.unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        session.commands.send(vec!["pause".into()]).await.unwrap();
        assert_eq!(updates.recv().await.unwrap().0.unwrap()["paused"], true);
        session
            .commands
            .send(vec![
                "history-action".into(),
                "demo/2".into(),
                "dismiss".into(),
            ])
            .await
            .unwrap();
        assert_eq!(
            updates.recv().await.unwrap().0.unwrap()["items"][1]["archived"],
            true
        );
        drop(session);
        tokio::time::timeout(Duration::from_secs(1), async {
            while updates.recv().await.is_ok() {}
        })
        .await
        .unwrap();
    }
}
