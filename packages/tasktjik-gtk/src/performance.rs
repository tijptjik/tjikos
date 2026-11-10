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
    assert!(
        view.navigation
            .borrow()
            .values()
            .filter(|w| w.upgrade().is_some())
            .count()
            < 80
    );
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
