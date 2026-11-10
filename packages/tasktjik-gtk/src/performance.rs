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
            .args(["-k", name])
            .status()
            .unwrap()
            .success());
        settle();
    };
    settle();
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
    view.update(&json!({"collection":"reading"}));
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
