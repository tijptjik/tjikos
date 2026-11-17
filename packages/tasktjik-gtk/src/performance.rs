//! Synthetic rendered collection measurements; run explicitly on a private display.
use super::*;
#[test]
#[ignore = "rendered collection profile; scripts/profile-collections.py"]
fn rendered_collection_profile() {
    gtk::init().unwrap();
    for collection in ["todo", "reading"] {
        for count in [100, 1000] {
            let view = TaskView::new(|_| {}, || {}, None);
            let window = gtk::Window::builder()
                .default_width(960)
                .default_height(720)
                .child(&view.root)
                .build();
            let tasks: Vec<Value> = (0..count)
                .map(|i| {
                    json!({
                        "id":format!("synthetic-{i}"), "kind":collection,
                        "title":format!("Synthetic record {i}"), "group":"All", "revision":1,
                        "reference":format!("https://example.invalid/{i}"), "tags":["fixture"],
                        "createdAt":"2026-01-01", "status":"pending", "images":[], "workflowRows":[]
                    })
                })
                .collect();
            let start = std::time::Instant::now();
            view.update(&json!({"tasks":tasks,"collection":collection}));
            let update = start.elapsed();
            window.present();
            let painted = Rc::new(Cell::new(false));
            let flag = painted.clone();
            let clock = window.frame_clock().unwrap();
            let signal = clock.connect_after_paint(move |_| flag.set(true));
            let context = glib::MainContext::default();
            while !painted.get() && start.elapsed() < std::time::Duration::from_secs(30) {
                context.iteration(false);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(painted.get(), "collection never painted");
            fn widgets(root: &gtk::Widget) -> usize {
                let mut count = 1;
                let mut child = root.first_child();
                while let Some(current) = child {
                    count += widgets(&current);
                    child = current.next_sibling();
                }
                count
            }
            println!(
                "collection={collection} records={count} update_ms={} first_paint_ms={} widgets={}",
                update.as_millis(),
                start.elapsed().as_millis(),
                widgets(view.root.upcast_ref())
            );
            clock.disconnect(signal);
            window.close();
            while context.pending() {
                context.iteration(false);
            }
        }
    }
}

#[test]
#[ignore = "requires private rendering display"]
fn task_virtualization_preserves_focus_and_revision() {
    gtk::init().unwrap();
    let sent = Rc::new(RefCell::new(Vec::new()));
    let requests = sent.clone();
    let view = TaskView::new(
        move |request| requests.borrow_mut().push(request),
        || {},
        None,
    );
    let tasks: Vec<_> = (0..2000)
        .map(|i| {
            json!({
                "id":format!("task-{i:04}"), "kind":"todo", "title":format!("Task {i:04}"),
                "group":format!("Group {}", i / 500), "revision":7,
                "status":"pending", "tags":[], "images":[], "workflowRows":[]
            })
        })
        .collect();
    view.update(&json!({"tasks":tasks,"collection":"todo"}));
    let window = gtk::Window::builder()
        .title("Tasktjik task fixture")
        .default_width(960)
        .default_height(650)
        .child(&view.root)
        .build();
    let settle = || {
        glib::MainContext::default()
            .block_on(glib::timeout_future(std::time::Duration::from_millis(500)))
    };
    window.present();
    settle();
    assert_eq!(view.order.borrow().len(), 2000);
    let live: Vec<_> = view
        .navigation
        .borrow()
        .values()
        .filter_map(|w| w.upgrade())
        .collect();
    eprintln!(
        "live task rows={} viewport={}",
        live.len(),
        view.scroll.height()
    );
    // Compact task cards fit more rows than the former action-heavy cards. Check
    // the actual overscan region as well as a bounded fraction of the 2,000 rows.
    assert!(
        live.len() < 200,
        "only a viewport and overscan may be materialized: {}",
        live.len()
    );
    for row in &live {
        let bounds = row.compute_bounds(&view.scroll).unwrap();
        assert!(
            bounds.y() + bounds.height() >= -(view.scroll.height() as f32) - 1.
                && bounds.y() <= view.scroll.height() as f32 * 2. + 1.,
            "row outside virtual overscan: {bounds:?}"
        );
    }
    drop(live);
    view.focus_task("task-1999");
    settle();
    settle();
    let card = view
        .navigation
        .borrow()
        .get("task-1999")
        .and_then(|w| w.upgrade())
        .unwrap();
    let bounds = card.compute_bounds(&view.scroll).unwrap();
    eprintln!(
        "last task bounds={bounds:?} viewport={}",
        view.scroll.height()
    );
    assert!(card.has_focus());
    assert!(bounds.y() >= -1. && bounds.y() + bounds.height() <= view.scroll.height() as f32 + 1.);
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
    let command = format!("hl.dispatch(hl.dsp.window.resize({{x=600,y=650,relative=false,window=\"address:{address}\"}}))");
    let output = std::process::Command::new("hyprctl")
        .args(["--instance", &instance, "eval", &command])
        .output()
        .unwrap();
    assert!(output.status.success());
    settle();
    settle();
    assert!(window.width() <= 620, "fixture must actually resize");
    // Crossing the domain-column breakpoint reconstructs the board. Verify the
    // current row and logical selection, not the detached pre-resize widget.
    let card = view
        .navigation
        .borrow()
        .get("task-1999")
        .and_then(|w| w.upgrade())
        .unwrap();
    assert!(card.has_focus());
    let bounds = card.compute_bounds(&view.scroll).unwrap();
    assert!(
        bounds.y() >= -1. && bounds.y() + bounds.height() <= view.scroll.height() as f32 + 1.,
        "resized selected card must remain visible: {bounds:?}"
    );
    let task = view.selected_task().unwrap();
    assert_eq!(task["id"], "task-1999");
    view.action("toggle", &task, json!({}));
    assert_eq!(sent.borrow().last().unwrap()["id"], "task-1999");
    assert_eq!(sent.borrow().last().unwrap()["revision"], 7);
    view.pending.set(false);
    view.focus_task("task-0000");
    settle();
    settle();
    assert!(view
        .navigation
        .borrow()
        .get("task-0000")
        .and_then(|w| w.upgrade())
        .unwrap()
        .has_focus());
    drop(card);
    assert!(view
        .navigation
        .borrow()
        .get("task-1999")
        .and_then(|w| w.upgrade())
        .is_none());
    view.filter.set_text("Task 0010");
    settle();
    assert_eq!(view.order.borrow().len(), 1);
    assert_eq!(view.order.borrow()[0].0, "task-0010");
    window.close();
    settle();
}

#[test]
#[ignore = "rendered keyboard acceptance; scripts/test-previews.py"]
fn task_application_keyboard_and_host_boundary() {
    gtk::init().unwrap();
    let exits = Rc::new(Cell::new(0));
    let exit = exits.clone();
    let sent = Rc::new(RefCell::new(Vec::new()));
    let requests = sent.clone();
    let view = TaskView::new(
        move |r| requests.borrow_mut().push(r),
        move || exit.set(exit.get() + 1),
        None,
    );
    view.update(&json!({"today":"2026-11-10","collection":"todo","tasks":[
        {"id":"one","title":"Synthetic task","group":"All","kind":"todo","revision":7,"period":"once","due":"2026-11-11"}
    ]}));
    let window = gtk::Window::builder()
        .title("Tasktjik task fixture")
        .default_width(860)
        .default_height(680)
        .child(&view.root)
        .build();
    window.add_css_class("tjik");
    let css = tjik_gtk::theme::provider();
    tjik_gtk::theme::apply_with_css(
        &css,
        &Default::default(),
        "sans-serif",
        14,
        super::style::CSS,
    );
    window.present();
    let settle = || {
        glib::MainContext::default()
            .block_on(glib::timeout_future(std::time::Duration::from_millis(250)))
    };
    let key = |name: &str| {
        assert!(std::process::Command::new("wtype")
            .args(["-k", name, "-s", "30"])
            .status()
            .unwrap()
            .success());
        settle();
    };
    settle();
    let picked = window
        .pick(
            window.width() as f64 / 2.,
            window.height() as f64 - 30.,
            gtk::PickFlags::DEFAULT,
        )
        .expect("bottom gap is pickable");
    assert!(
        picked == view.scroll || picked.is_ancestor(&view.scroll),
        "the footer gap must pass input to task content: {}",
        picked.type_().name()
    );
    view.focus();
    let sequence = |letters: &str| {
        assert!(std::process::Command::new("wtype")
            .args(["-d", "100", letters])
            .status()
            .unwrap()
            .success());
        settle();
    };
    for (suffix, index) in [("td", 1), ("tw", 2), ("tm", 3), ("ta", 0)] {
        sequence(suffix);
        settle();
        assert!(!view.is_editing());
        assert_eq!(view.dates.selected(), index);
    }
    sequence("tp");
    assert_eq!(view.selector.selected(), 2);
    for (suffix, index) in [("mr", 1), ("mp", 2), ("mb", 3), ("mt", 0)] {
        sequence(suffix);
        assert_eq!(view.selector.selected(), index);
    }
    key("e");
    assert!(view.is_editing());
    key("Escape");
    view.focus();
    key("n");
    assert!(
        view.is_editing(),
        "N must open the editor from the task application"
    );
    key("Escape");
    assert!(!view.is_editing());
    assert_eq!(
        exits.get(),
        0,
        "editor Escape must not escape the application"
    );
    view.focus();
    key("slash");
    assert!(WidgetExt::is_visible(&view.filter));
    assert!(std::process::Command::new("wtype")
        .arg("n")
        .status()
        .unwrap()
        .success());
    settle();
    assert_eq!(view.filter.text().as_str(), "n");
    assert!(!view.is_editing(), "filter text must not invoke New");
    key("Escape");
    assert!(!WidgetExt::is_visible(&view.filter));
    assert_eq!(exits.get(), 0);
    view.focus_task("one");
    key("space");
    let request = sent.borrow().last().unwrap().clone();
    assert_eq!(request["id"], "one");
    assert_eq!(request["revision"], 7);
    assert!(
        request.get("op").is_none() && request.get("provider").is_none(),
        "launcher routing belongs to its host"
    );
    view.pending.set(false);
    view.root.grab_focus();
    key("Escape");
    assert_eq!(exits.get(), 1);
    view.update(&json!({"collection":"reading", "message":"Opened reading reference"}));
    assert!(
        !WidgetExt::is_visible(&view.message),
        "Reading hides success statuses"
    );
    view.focus();
    key("Escape");
    assert_eq!(
        exits.get(),
        2,
        "reading and tasks use the same host exit callback"
    );
    window.close();
    settle();
}

#[test]
#[ignore = "requires an isolated rendering display"]
fn completion_draws_then_fades_then_reflows() {
    gtk::init().unwrap();
    let settle = |ms| {
        glib::MainContext::default()
            .block_on(glib::timeout_future(std::time::Duration::from_millis(ms)))
    };
    let view = TaskView::new(|_| {}, || {}, None);
    let window = gtk::Window::builder()
        .default_width(960)
        .default_height(720)
        .child(&view.root)
        .build();
    let mut tasks = json!([
        {"id":"a","kind":"todo","title":"A","group":"All","done":false},
        {"id":"b","kind":"todo","title":"B","group":"All","done":false}
    ]);
    view.update(&json!({"tasks":tasks,"collection":"todo"}));
    window.present();
    settle(350);
    let first = view.navigation.borrow()["a"].upgrade().unwrap();
    let second = view.navigation.borrow()["b"].upgrade().unwrap();
    let old_y = second.compute_bounds(&view.scroll).unwrap().y();
    tasks[0]["done"] = json!(true);
    view.update(&json!({"tasks":tasks,"mutation_accepted":true}));
    assert!(view.completing.get());
    settle(240);
    assert_eq!(first.opacity(), 1.);
    assert_eq!(second.compute_bounds(&view.scroll).unwrap().y(), old_y);
    settle(110);
    assert!(first.opacity() > 0. && first.opacity() < 1.);
    assert_eq!(second.compute_bounds(&view.scroll).unwrap().y(), old_y);
    settle(450);
    assert!(!view.completing.get());
    assert!(!view.navigation.borrow().contains_key("a"));
    let second = view.navigation.borrow()["b"].upgrade().unwrap();
    assert!(second.compute_bounds(&view.scroll).unwrap().y() < old_y);
    // Included completions keep their row and full opacity after the check draws.
    view.completed.set_mode(1);
    tasks[1]["done"] = json!(true);
    view.update(&json!({"tasks":tasks,"mutation_accepted":true}));
    settle(700);
    assert_eq!(
        view.navigation.borrow()["b"].upgrade().unwrap().opacity(),
        1.
    );
    window.close();
}

#[test]
#[ignore = "requires an isolated rendering display"]
fn optimistic_actions_queue_without_waiting_for_server() {
    gtk::init().unwrap();
    let sent = Rc::new(RefCell::new(Vec::<Value>::new()));
    let outgoing = sent.clone();
    let view = TaskView::new(move |r| outgoing.borrow_mut().push(r), || {}, None);
    view.configure_mutations(|state, request| {
        let mut state = state.clone();
        let Some(task) = state["tasks"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|t| t["id"] == request["id"])
        else {
            return Ok(state);
        };
        if request["action"] == "toggle" {
            task["done"] = json!(task["done"] != true);
        }
        if let Some(fields) = request["fields"].as_object() {
            for (k, v) in fields {
                task[k] = v.clone();
            }
        }
        task["revision"] = json!(task["revision"].as_u64().unwrap() + 1);
        Ok(state)
    });
    let outbox_directory = tempfile::tempdir().unwrap();
    let outbox = outbox_directory.path().join("outbox.json");
    view.configure_outbox(outbox.clone()).unwrap();
    let task = json!({"id":"a","kind":"todo","title":"A","group":"All","revision":1,"done":false});
    view.update(&json!({"tasks":[task],"collection":"todo"}));
    let window = gtk::Window::builder()
        .default_width(960)
        .default_height(720)
        .child(&view.root)
        .build();
    window.present();
    let settle = |ms| {
        glib::MainContext::default()
            .block_on(glib::timeout_future(std::time::Duration::from_millis(ms)))
    };
    settle(300);
    view.action("toggle", &task, json!({}));
    assert_eq!(view.state()["tasks"][0]["done"], true);
    assert!(
        view.completing.get(),
        "check animation starts before any response"
    );
    view.action(
        "edit",
        &task,
        json!({"fields":{"title":"Changed while saving"}}),
    );
    let persisted: Value = serde_json::from_slice(&std::fs::read(&outbox).unwrap()).unwrap();
    assert_eq!(
        persisted["queue"].as_array().unwrap().len(),
        2,
        "persist the second write while the first is in flight"
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&outbox).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        sent.borrow().len(),
        1,
        "serialize transport, not user actions"
    );
    assert_eq!(view.state()["tasks"][0]["title"], "Changed while saving");
    settle(800);
    assert!(
        !view.navigation.borrow().contains_key("a"),
        "completion finishes without server acknowledgement"
    );
    let first = sent.borrow()[0]["request_id"].as_str().unwrap().to_owned();
    view.response(&first, Ok(json!({"tasks":[{"id":"a","kind":"todo","title":"A","group":"All","revision":2,"done":true}],"mutation_accepted":true})));
    assert_eq!(sent.borrow().len(), 2);
    assert_eq!(sent.borrow()[1]["revision"], 2);
    assert_eq!(view.state()["tasks"][0]["title"], "Changed while saving");
    let second = sent.borrow()[1]["request_id"].as_str().unwrap().to_owned();
    view.response(&second, Err("Synthetic rejection".into()));
    assert_eq!(view.state()["tasks"][0]["title"], "A");
    assert_eq!(
        view.state()["tasks"][0]["done"],
        true,
        "do not undo successful predecessor"
    );
    assert!(view.recover.is_visible());
    assert!(!view.has_queued_updates());
    view.action("edit", &task, json!({"fields":{"title":"Queued offline"}}));
    let retry_id = sent.borrow().last().unwrap()["request_id"]
        .as_str()
        .unwrap()
        .to_owned();
    view.response(
        &retry_id,
        Ok(json!({"retryable_error":"Synthetic connection loss"})),
    );
    assert_eq!(view.state()["tasks"][0]["title"], "Queued offline");
    assert!(view.has_queued_updates());
    view.dispatch_next();
    assert_eq!(
        sent.borrow().last().unwrap()["request_id"],
        retry_id,
        "retry the same idempotent write"
    );
    view.response(&retry_id, Err("Synthetic explicit rejection".into()));
    assert_eq!(view.state()["tasks"][0]["title"], "A");
    window.close();
}

#[test]
#[ignore = "requires an isolated rendering display"]
fn snooze_expires_without_server_refresh() {
    gtk::init().unwrap();
    let view = TaskView::new(
        |_| panic!("expiry must not mutate server data"),
        || {},
        None,
    );
    let until = (chrono::Utc::now() + chrono::Duration::seconds(2)).to_rfc3339();
    view.update(&json!({"tasks":[{"id":"wake","kind":"todo","title":"Wake fixture","group":"All","snoozedUntil":until,"due":""}]}));
    let window = gtk::Window::builder()
        .default_width(860)
        .default_height(680)
        .child(&view.root)
        .build();
    window.present();
    let settle = |ms| {
        glib::MainContext::default()
            .block_on(glib::timeout_future(std::time::Duration::from_millis(ms)))
    };
    settle(200);
    assert!(view.markers.borrow()["wake"]
        .button
        .tooltip_text()
        .unwrap()
        .contains("Snoozed until"));
    settle(4000);
    assert_eq!(
        view.markers.borrow()["wake"]
            .button
            .tooltip_text()
            .as_deref(),
        Some("Complete")
    );
    assert_eq!(view.state()["tasks"][0]["snoozedUntil"], until);
    window.close();
}

#[test]
#[ignore = "requires an isolated rendering display"]
fn responsive_headers_and_reading_dates() {
    gtk::init().unwrap();
    let view = TaskView::new(|_| {}, || {}, None);
    let window = gtk::Window::builder()
        .title("Tasktjik task fixture")
        .default_width(760)
        .default_height(640)
        .child(&view.root)
        .build();
    window.add_css_class("tjik");
    let css = tjik_gtk::theme::provider();
    tjik_gtk::theme::apply_with_css(
        &css,
        &Default::default(),
        "sans-serif",
        14,
        super::style::CSS,
    );
    view.update(&json!({"today":"2026-11-17","collection":"todo","message":"Opened reading reference","tasks":[
        {"id":"task","kind":"todo","title":"Prepare the neighbourhood guide","group":"Inbox","due":"2026-11-17"},
        {"id":"page","kind":"reading","title":"A field guide to quieter interfaces","reference":"https://example.test/design","group":"All","due":"2026-11-17"},
        {"id":"later","kind":"reading","title":"Reading for next month","reference":"https://example.test/later","group":"All","snoozedUntil":"2099-02-17T09:00:00+08:00"}
    ]}));
    window.present();
    let settle = || {
        glib::MainContext::default()
            .block_on(glib::timeout_future(std::time::Duration::from_millis(350)))
    };
    settle();
    assert!(view.root.has_css_class("compact-task-header"));
    assert!(!WidgetExt::is_visible(&view.message));
    assert!(!WidgetExt::is_visible(&view.options));
    assert!(
        view.heading.borrow().is_ancestor(&view.toolbar),
        "Title belongs in the toolbar"
    );
    let sort = view.sort.compute_bounds(&view.toolbar).unwrap();
    let selector = view.selector.compute_bounds(&view.toolbar).unwrap();
    assert!(sort.x() > selector.x());
    assert_eq!(sort.y(), selector.y());
    let capture = |name: &str| {
        if let Ok(directory) = std::env::var("TASKTJIK_LAYOUT_ARTIFACTS") {
            std::fs::create_dir_all(&directory).unwrap();
            std::process::Command::new("hyprctl")
                .arg("dismissnotify")
                .output()
                .unwrap();
            assert!(std::process::Command::new("grim")
                .arg(format!("{directory}/{name}.png"))
                .status()
                .unwrap()
                .success());
        }
    };
    capture("tasks-compact");
    view.selector.set_selected(1);
    settle();
    capture("reading-compact");
    assert!(std::process::Command::new("hyprctl")
        .args([
            "eval",
            "hl.monitor({output=\"GTK\",mode=\"1400x800@60\",position=\"0x0\",scale=1})"
        ])
        .status()
        .unwrap()
        .success());
    window.set_child(None::<&gtk::Widget>);
    window.close();
    settle();
    let window = gtk::Window::builder()
        .title("Tasktjik task fixture")
        .default_width(1200)
        .default_height(640)
        .child(&view.root)
        .build();
    window.set_size_request(1200, 640);
    window.add_css_class("tjik");
    window.present();
    settle();
    assert!(
        view.root.has_css_class("compact-task-header"),
        "window width {} root width {}",
        window.width(),
        view.root.width()
    );
    capture("reading-wide");
    view.selector.set_selected(0);
    settle();
    assert!(!WidgetExt::is_visible(&view.options));
    capture("tasks-wide");
    assert!(std::process::Command::new("hyprctl")
        .args([
            "eval",
            "hl.monitor({output=\"GTK\",mode=\"760x680@60\",position=\"0x0\",scale=1})"
        ])
        .status()
        .unwrap()
        .success());
    window.set_child(None::<&gtk::Widget>);
    window.close();
    settle();
    let window = gtk::Window::builder()
        .title("Tasktjik task fixture")
        .default_width(760)
        .default_height(640)
        .child(&view.root)
        .build();
    window.add_css_class("tjik");
    window.present();
    settle();
    settle();
    capture("tasks-narrow");
    assert!(view.root.has_css_class("compact-task-header"));
    assert!(!WidgetExt::is_visible(&view.options));
    window.close();
}

#[test]
#[ignore = "rendered clipboard interaction; scripts/test-reading.py"]
fn task_editor_pastes_text_and_images() {
    gtk::init().unwrap();
    let sent = Rc::new(RefCell::new(Vec::new()));
    let requests = sent.clone();
    let view = TaskView::new(move |r| requests.borrow_mut().push(r), || {}, None);
    let task =
        json!({"id":"paste-fixture","title":"Original","kind":"todo","group":"All","revision":7});
    view.update(&json!({"collection":"todo","tasks":[task.clone()]}));
    let window = gtk::Window::builder()
        .title("Tasktjik task fixture")
        .default_width(760)
        .default_height(640)
        .child(&view.root)
        .build();
    window.present();
    let settle = || {
        glib::MainContext::default()
            .block_on(glib::timeout_future(std::time::Duration::from_millis(350)))
    };
    settle();
    view.edit(task.clone());
    settle();
    fn entry(widget: &gtk::Widget) -> Option<gtk::Entry> {
        if widget.widget_name() == "Title" {
            return widget.clone().downcast().ok();
        }
        let mut child = widget.first_child();
        while let Some(w) = child {
            if let Some(found) = entry(&w) {
                return Some(found);
            }
            child = w.next_sibling();
        }
        None
    }
    let title = entry(view.root.upcast_ref()).unwrap();
    title.grab_focus();
    title.select_region(0, -1);
    view.root
        .clipboard()
        .set_text("Pasted title https://example.test");
    let paste = || {
        assert!(std::process::Command::new("wtype")
            .args(["-M", "ctrl", "-k", "v", "-m", "ctrl"])
            .status()
            .unwrap()
            .success());
        settle();
    };
    paste();
    assert_eq!(title.text(), "Pasted title https://example.test");
    assert!(
        sent.borrow().is_empty(),
        "text paste must not send an image mutation"
    );
    let texture = gdk::MemoryTexture::new(
        1,
        1,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(vec![20u8, 40, 60, 255]),
        4,
    );
    view.root.clipboard().set_texture(&texture);
    paste();
    assert_eq!(sent.borrow().len(), 1);
    assert_eq!(sent.borrow()[0]["action"], "attach-image");
    assert_eq!(sent.borrow()[0]["id"], "paste-fixture");
    assert_eq!(sent.borrow()[0]["revision"], 7);
    assert!(sent.borrow()[0]["data"]
        .as_str()
        .unwrap()
        .starts_with("iVBOR"));
    assert_eq!(title.text(), "Pasted title https://example.test");
    assert!(view.is_editing(), "pasting must preserve the open draft");
    window.close();
}

#[test]
#[ignore = "rendered project navigation; scripts/test-reading.py"]
fn projects_follow_filters_and_back_navigation() {
    gtk::init().unwrap();
    let view = TaskView::new(|_| {}, || {}, None);
    view.update(&json!({"today":"2026-11-17","collection":"todo","tasks":[
        {"id":"active","kind":"project","title":"Active","group":"Inbox","onlyInProject":true},
        {"id":"finished","kind":"project","title":"Finished","group":"Inbox","onlyInProject":true},
        {"id":"future","kind":"project","title":"Future","group":"Inbox","onlyInProject":true},
        {"id":"empty","kind":"project","title":"Empty","group":"Inbox"},
        {"id":"a","kind":"todo","project":"active","title":"Today","group":"Inbox","due":"2026-11-17"},
        {"id":"b","kind":"todo","project":"finished","title":"Done","group":"Inbox","due":"2026-11-17","done":true},
        {"id":"c","kind":"todo","project":"future","title":"Later","group":"Inbox","due":"2099-12-01"}
    ]}));
    let window = gtk::Window::builder()
        .title("Tasktjik task fixture")
        .default_width(760)
        .default_height(640)
        .child(&view.root)
        .build();
    window.present();
    let settle = || {
        glib::MainContext::default()
            .block_on(glib::timeout_future(std::time::Duration::from_millis(350)))
    };
    settle();
    let visible = |id: &str| view.order.borrow().iter().any(|(key, _, _)| key == id);
    view.dates.set_selected(1);
    settle();
    assert!(visible("active"));
    for id in ["finished", "future", "empty"] {
        assert!(!visible(id), "Unexpected project {id}");
    }
    view.completed.button.emit_clicked();
    settle();
    assert!(visible("active") && visible("finished"));
    view.completed.button.emit_clicked();
    settle();
    assert!(!visible("active") && visible("finished"));
    view.completed.button.emit_clicked();
    view.selector.set_selected(2);
    view.focus_task("active");
    assert!(std::process::Command::new("wtype")
        .args(["-k", "Return"])
        .status()
        .unwrap()
        .success());
    settle();
    assert_eq!(view.project.borrow().as_str(), "active");
    assert!(visible("a"), "Projects mode must show the project's tasks");
    assert!(std::process::Command::new("wtype")
        .args(["-k", "BackSpace"])
        .status()
        .unwrap()
        .success());
    settle();
    assert!(view.project.borrow().is_empty());
    assert_eq!(view.selector.selected(), 2);
    assert_eq!(view.dates.selected(), 1);
    assert!(visible("active"));
    *view.project.borrow_mut() = "active".into();
    view.render();
    let controllers = view.root.observe_controllers();
    let back = (0..controllers.n_items())
        .filter_map(|i| controllers.item(i).and_downcast::<gtk::GestureClick>())
        .find(|g| g.button() == 8)
        .expect("mouse Back controller");
    back.emit_by_name::<()>("pressed", &[&1i32, &0f64, &0f64]);
    assert!(view.project.borrow().is_empty());
    view.dates.set_selected(0);
    settle();
    assert!(
        visible("empty") && visible("future"),
        "All dates retains the project directory"
    );
    window.close();
}

#[test]
#[ignore = "rendered project settings; scripts/test-reading.py"]
fn project_incoming_category_routes_only_inbox() {
    gtk::init().unwrap();
    for (source, default, expected) in [
        ("All", "Work", "Work"),
        ("Personal", "Work", "Personal"),
        ("All", "", "All"),
    ] {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let requests = sent.clone();
        let view = TaskView::new(move |r| requests.borrow_mut().push(r), || {}, None);
        let task =
            json!({"id":"task","kind":"todo","title":"Synthetic","group":source,"revision":3});
        view.update(&json!({"collection":"todo","tasks":[task.clone(), {"id":"project","kind":"project","title":"Project","group":"Projects","metadata":{"projectIncomingCategory":default}}]}));
        view.drop_task(&task, "Projects", "", Some("project"));
        assert_eq!(sent.borrow()[0]["fields"]["group"], expected);
        assert_eq!(sent.borrow()[0]["fields"]["project"], "project");
        assert_eq!(sent.borrow()[0]["revision"], 3);
    }
    let sent = Rc::new(RefCell::new(Vec::new()));
    let requests = sent.clone();
    let view = TaskView::new(move |r| requests.borrow_mut().push(r), || {}, None);
    let project = json!({"id":"project","kind":"project","title":"Project","group":"All","revision":4,"metadata":{"keep":"unchanged"}});
    view.update(&json!({"collection":"project","groups":["All","Work"],"tasks":[project.clone()]}));
    let window = gtk::Window::builder()
        .title("Tasktjik task fixture")
        .default_width(760)
        .default_height(640)
        .child(&view.root)
        .build();
    window.present();
    view.edit(project);
    fn find(widget: &gtk::Widget) -> Option<gtk::DropDown> {
        if widget.widget_name() == "project-incoming-category" {
            return widget.clone().downcast().ok();
        }
        let mut child = widget.first_child();
        while let Some(w) = child {
            if let Some(found) = find(&w) {
                return Some(found);
            }
            child = w.next_sibling();
        }
        None
    }
    find(view.root.upcast_ref()).unwrap().set_selected(1);
    let settle = || {
        glib::MainContext::default()
            .block_on(glib::timeout_future(std::time::Duration::from_millis(350)))
    };
    settle();
    assert!(std::process::Command::new("wtype")
        .args(["-M", "ctrl", "-k", "Return", "-m", "ctrl"])
        .status()
        .unwrap()
        .success());
    settle();
    assert_eq!(
        sent.borrow()[0]["fields"]["metadata"]["projectIncomingCategory"],
        "Work"
    );
    assert_eq!(sent.borrow()[0]["fields"]["metadata"]["keep"], "unchanged");
    window.close();
}
