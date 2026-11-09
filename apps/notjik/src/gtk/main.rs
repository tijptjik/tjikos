use clap::Parser;
use gtk::{gio, glib, prelude::*};
use notjik_core::gtk::{self as cards, model::*, primitives::*, Card};
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};
#[derive(Parser)]
#[command(about = "Synthetic GTK card gallery; does not own the notification bus or send actions")]
struct Options {
    #[arg(long, required = true)]
    demo: bool,
    #[arg(long,default_value="rose-punk",value_parser=["rose-punk","monochrome"])]
    theme: String,
    #[arg(long)]
    media_root: Option<PathBuf>,
    #[arg(long)]
    self_test: bool,
    #[arg(long)]
    screenshot: Option<PathBuf>,
    #[arg(long)]
    snapshot_dir: Option<PathBuf>,
    #[arg(long, default_value_t = 460)]
    width: i32,
}
fn main() -> glib::ExitCode {
    let opts = Rc::new(Options::parse());
    if opts.self_test {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            default(info);
            std::process::exit(1);
        }));
    }
    let app = gtk::Application::builder()
        .application_id("org.tjikos.notjik.cards")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        let root = std::env::var_os("NOTJIK_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let exe = std::env::current_exe().unwrap();
                let installed = exe.parent().unwrap().parent().unwrap();
                if installed.join("themes").is_dir() {
                    installed.into()
                } else {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                }
            });
        let style: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("themes").join(format!("{}.json", opts.theme))).unwrap(),
        )
        .unwrap();
        let _provider = apply_theme(&style);
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Notjik GTK cards · synthetic demo")
            .default_width(opts.width)
            .default_height(840)
            .build();
        window.add_css_class("notjik");
        window.set_resizable(false);
        let outer = column(12);
        outer.add_css_class("gallery");
        outer.append(&label("Notjik · GTK cards", "card-title"));
        outer.append(&label(
            "Synthetic preview · actions are simulated",
            "caption",
        ));
        let controls = row(8);
        let lock = gtk::ToggleButton::with_label("Lock preview");
        lock.set_widget_name("demo-lock");
        controls.append(&lock);
        outer.append(&controls);
        let log = label(
            "Hover, copy, reply, snooze, or use a card action.",
            "caption",
        );
        outer.append(&log);
        let list = column(14);
        outer.append(&list);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        scroll.set_child(Some(&outer));
        window.set_child(Some(&scroll));
        let all: Rc<RefCell<Vec<Rc<Card>>>> = Rc::new(RefCell::new(vec![]));
        let events = Rc::new(RefCell::new(Vec::<CardEvent>::new()));
        let now = chrono::Utc::now().timestamp_millis() as f64;
        for notice in cards::fixtures::notices(&root, now, opts.media_root.as_deref()) {
            let weak = Rc::downgrade(&all);
            let log = log.clone();
            let events = events.clone();
            let card = Rc::new(Card::new(
                notice,
                ViewState {
                    locked: false,
                    ..Default::default()
                },
                &root,
                now,
                move |event| {
                    events.borrow_mut().push(event.clone());
                    if matches!(
                        event.intent,
                        Intent::Hover(_) | Intent::Hold(_) | Intent::Select | Intent::Drag { .. }
                    ) {
                        return;
                    }
                    log.set_text(&format!("Demo only · {:?}", event.intent));
                    let weak = weak.clone();
                    // Host updates occur after the current widget signal returns.
                    glib::idle_add_local_once(move || {
                        let Some(all) = weak.upgrade() else {
                            return;
                        };
                        let card = all
                            .borrow()
                            .iter()
                            .find(|c| c.notice().id() == event.id)
                            .cloned();
                        let Some(card) = card else {
                            return;
                        };
                        let mut state = card.state();
                        match event.intent {
                            Intent::PlaybackToggle => {
                                state.playback.playing = !state.playback.playing;
                                state.playback.duration_ms = 8000;
                                state.playback.seekable = true;
                            }
                            Intent::PlaybackRate(rate) => state.playback.rate = rate,
                            Intent::Seek(ms) => state.playback.position_ms = ms,
                            Intent::Record => {
                                state.recording.phase = "recording".into();
                                state.recording.duration_ms = 1000;
                            }
                            Intent::RecordDiscard => state.recording = Default::default(),
                            Intent::RecordSend => {
                                state.recording = Default::default();
                                state.feedback =
                                    "Synthetic voice reply sent · no microphone or account used"
                                        .into();
                            }
                            Intent::Telegram { action, .. } if action == "transcribe" => {
                                let mut n = card.notice();
                                n.0["data"]["transcript"] = serde_json::json!(
                                    "See you at the café tomorrow. This is a synthetic transcript."
                                );
                                n.0["data"]["transcriptionState"] = serde_json::json!("done");
                                card.update(n);
                            }
                            Intent::Telegram { action, .. } if action == "reply" => {
                                card.clear_reply();
                                state.feedback =
                                    "Synthetic reply sent · no Telegram message sent".into();
                            }
                            Intent::Dismiss => {
                                card.widget.set_visible(false);
                                return;
                            }
                            _ => {
                                state.feedback =
                                    "Demo action accepted · nothing sent or executed".into()
                            }
                        }
                        card.set_state(state);
                    });
                },
            ));
            list.append(&card.widget);
            all.borrow_mut().push(card);
        }
        let weak = Rc::downgrade(&all);
        lock.connect_toggled(move |b| {
            if let Some(all) = weak.upgrade() {
                for card in all.borrow().iter() {
                    let mut state = card.state();
                    state.locked = b.is_active();
                    card.set_state(state);
                }
            }
        });
        let capture = opts.self_test;
        let cards_tick = all.clone();
        let weak_window = window.downgrade();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            if weak_window.upgrade().is_none() {
                return glib::ControlFlow::Break;
            }
            let now = chrono::Utc::now().timestamp_millis() as f64;
            if !capture {
                for card in cards_tick.borrow().iter() {
                    card.tick(now);
                }
            }
            glib::ControlFlow::Continue
        });
        window.present();
        lock.grab_focus();
        let options = opts.clone();
        let app = app.clone();
        let window = window.clone();
        glib::MainContext::default().spawn_local(async move {
            glib::timeout_future(Duration::from_millis(600)).await;
            if let Some(dir) = &options.snapshot_dir {
                std::fs::create_dir_all(dir).expect("snapshot directory");
                let snapshots = all.borrow().clone();
                std::fs::write(
                    dir.join("fixtures.json"),
                    serde_json::to_vec_pretty(
                        &snapshots.iter().map(|c| c.notice().0).collect::<Vec<_>>(),
                    )
                    .unwrap(),
                )
                .unwrap();
                for card in &snapshots {
                    card.set_animations_active(false);
                    card.widget.set_visible(false);
                }
                glib::timeout_future(Duration::from_millis(200)).await;
                for (index, card) in snapshots.iter().enumerate() {
                    lock.grab_focus();
                    card.set_hovered(false);
                    card.widget.set_visible(true);
                    card.set_animations_active(true);
                    let before = card.media_status();
                    glib::timeout_future(Duration::from_millis(450)).await;
                    let after = card.media_status();
                    for (first, last) in before.iter().zip(&after) {
                        assert!(
                            last.ready && last.error.is_empty(),
                            "media {index}: {last:?}"
                        );
                        let notice = card.notice();
                        if ["gif", "video"]
                            .contains(&notice.data()["format"].as_str().unwrap_or(""))
                            || notice.data()["parts"].is_array()
                        {
                            assert!(
                                last.frames > first.frames,
                                "media {index} did not advance: {first:?} -> {last:?}"
                            );
                        }
                    }
                    lock.grab_focus();
                    card.set_hovered(false);
                    glib::timeout_future(Duration::from_millis(220)).await;
                    let paintable = gtk::WidgetPaintable::new(Some(&card.widget));
                    scroll.vadjustment().set_value(0.);
                    card.widget.queue_draw();
                    card.set_animations_active(false);
                    glib::timeout_future(Duration::from_millis(150)).await;
                    assert!(
                        card.widget.width() <= options.width,
                        "card exceeds gallery width"
                    );
                    assert!(
                        card.widget.height() < 600,
                        "card stretched beyond its content"
                    );
                    let snapshot = gtk::Snapshot::new();
                    paintable.snapshot(
                        &snapshot,
                        card.widget.width() as f64,
                        card.widget.height() as f64,
                    );
                    let node = snapshot.to_node().expect("card render node");
                    let texture = window.renderer().unwrap().render_texture(&node, None);
                    texture
                        .save_to_png(dir.join(format!("card-{index:02}.png")))
                        .expect("card screenshot");
                    card.set_hovered(true);
                    glib::timeout_future(Duration::from_millis(220)).await;
                    let shot = gtk::Snapshot::new();
                    paintable.snapshot(
                        &shot,
                        card.widget.width() as f64,
                        card.widget.height() as f64,
                    );
                    let node = shot.to_node().expect("revealed card render node");
                    window
                        .renderer()
                        .unwrap()
                        .render_texture(&node, None)
                        .save_to_png(dir.join(format!("revealed-{index:02}.png")))
                        .expect("revealed screenshot");
                    card.set_hovered(false);
                    card.widget.set_visible(false);
                }
                for card in &snapshots {
                    card.widget.set_visible(true);
                }
            }
            scroll.vadjustment().set_value(0.);
            glib::timeout_future(Duration::from_millis(150)).await;
            if let Some(path) = &options.screenshot {
                // Capture this gallery's widget tree, never the user's desktop.
                let paintable = gtk::WidgetPaintable::new(Some(&window));
                let snapshot = gtk::Snapshot::new();
                paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
                let node = snapshot.to_node().expect("gallery render node");
                let renderer = window.renderer().expect("gallery renderer");
                let texture = renderer.render_texture(&node, None);
                texture.save_to_png(path).expect("save gallery screenshot");
            }
            if options.self_test {
                for card in all.borrow().iter() {
                    card.set_hovered(true);
                }
                glib::timeout_future(Duration::from_millis(220)).await;
                self_test(&all, &events);
                app.quit();
            }
        });
    });
    app.run_with_args::<&str>(&[])
}
fn find(widget: &impl IsA<gtk::Widget>, name: &str) -> Option<gtk::Widget> {
    let widget = widget.as_ref();
    if widget.widget_name() == name {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(w) = child {
        if let Some(found) = find(&w, name) {
            return Some(found);
        }
        child = w.next_sibling();
    }
    None
}
fn click(card: &Card, name: &str) {
    card.set_hovered(true);
    let button = find(&card.widget, name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .downcast::<gtk::Button>()
        .unwrap();
    assert!(button.is_sensitive(), "disabled {name}");
    button.emit_clicked();
}
fn self_test(all: &Rc<RefCell<Vec<Rc<Card>>>>, events: &Rc<RefCell<Vec<CardEvent>>>) {
    let cards = all.borrow();
    assert_eq!(cards.len(), 19);
    let reveal = find(&cards[8].widget, "telegram-controls-reveal")
        .unwrap()
        .downcast::<gtk::Revealer>()
        .unwrap();
    cards[8].set_hovered(false);
    assert!(!reveal.reveals_child());
    cards[8].set_hovered(true);
    assert!(reveal.reveals_child());
    let play = find(&cards[10].widget, "voice-play")
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap();
    assert!(play.has_css_class("voice-play"));
    assert!(play.label().is_none());
    assert!(play.is_sensitive());
    let read = find(&cards[8].widget, "telegram-read")
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap();
    assert!(read.label().is_none());
    assert!(read.child().unwrap().is::<gtk::DrawingArea>());
    for card in cards.iter() {
        assert!(card.widget.width() > 0);
        assert!(card.widget.height() > 0);
    }
    let timer = &cards[3];
    let start = timer.notice().0["received"].as_f64().unwrap();
    timer.tick(start + 31_000.);
    assert_eq!(
        find(&timer.widget, "countdown-value")
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap()
            .text(),
        "00:00"
    );
    let progress = &cards[2];
    let mut update = progress.notice();
    update.0["data"]["value"] = serde_json::json!(150);
    progress.update(update);
    assert_eq!(
        find(&progress.widget, "progress-value")
            .unwrap()
            .downcast::<gtk::ProgressBar>()
            .unwrap()
            .fraction(),
        1.
    );
    click(&cards[0], "card-action-0");
    assert_eq!(events.borrow().last().unwrap().intent, Intent::Invoke(0));
    let task = &cards[7];
    click(task, "todo-snooze");
    click(task, "snooze-tomorrow");
    assert!(
        matches!(&events.borrow().last().unwrap().intent,Intent::Task{action,until:Some(_)} if action=="snooze")
    );
    let telegram = &cards[8];
    click(telegram, "telegram-quote");
    let input = find(&telegram.widget, "telegram-reply")
        .unwrap()
        .downcast::<gtk::TextView>()
        .unwrap();
    input.buffer().set_text("Synthetic quoted reply");
    let mut updated = telegram.notice();
    updated.0["data"]["messageId"] = serde_json::json!("new-message");
    telegram.update(updated);
    assert_eq!(telegram.draft(), "Synthetic quoted reply");
    click(telegram, "telegram-send");
    assert!(
        matches!(&events.borrow().last().unwrap().intent,Intent::Telegram{quote:true,message_id,text,..} if message_id=="9" && text=="Synthetic quoted reply")
    );
    let before = events.borrow().len();
    let mut state = telegram.state();
    state.locked = true;
    telegram.set_state(state);
    assert!(!telegram.widget.is_visible());
    find(&telegram.widget, "telegram-send")
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap()
        .emit_clicked();
    assert_eq!(events.borrow().len(), before);
    let mut state = telegram.state();
    state.locked = false;
    state.history = true;
    telegram.set_state(state);
    assert!(!find(&telegram.widget, "telegram-send")
        .unwrap()
        .is_sensitive());
    let mut state = telegram.state();
    state.history = false;
    state.recording.phase = "ready".into();
    state.recording.uncertain = true;
    telegram.set_state(state);
    assert!(!find(&telegram.widget, "record-send")
        .unwrap()
        .is_sensitive());
    let mut state = telegram.state();
    state.recording = Default::default();
    telegram.set_state(state);
    let mut replacement = telegram.notice();
    replacement.0["id"] = serde_json::json!("replacement");
    telegram.update(replacement);
    assert!(telegram.draft().is_empty());
    let text = cards::clipboard::markdown(&cards[4].notice(), 0.);
    assert!(text.contains("Sectors mapped"));
    assert!(!text.contains("synthetic"));
    println!("GTK card acceptance: 19 variants; default media decoding/animation, icons, reveals, actions, snooze, quoted draft replacement, privacy, archive gating, uncertain recording and clipboard passed");
}
