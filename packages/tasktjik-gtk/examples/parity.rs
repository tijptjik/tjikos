//! Synthetic inspection host for comparing the shared view with the QML reference.
use serde_json::{json, Value};
use std::{cell::RefCell, path::PathBuf, rc::Rc};
use tjik_gtk::gtk::{self, glib, prelude::*};
fn main() {
    gtk::init().unwrap();
    let root = PathBuf::from(std::env::args().nth(1).expect("fixture directory"));
    let state: Value =
        serde_json::from_slice(&std::fs::read(root.join("fixture.json")).unwrap()).unwrap();
    let requests = Rc::new(RefCell::new(Vec::<Value>::new()));
    let sent = requests.clone();
    let view = tjik_task_gtk::TaskView::new(move |r| sent.borrow_mut().push(r), || {}, None);
    view.update(&state);
    let window = gtk::Window::builder()
        .title("Tasktjik parity fixture")
        .default_width(1000)
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
        tjik_task_gtk::style::CSS,
    );
    window.present();
    view.focus();
    let sequence = Rc::new(RefCell::new(String::new()));
    let main_loop = glib::MainLoop::new(None, false);
    let quit = main_loop.clone();
    window.connect_close_request(move |_| {
        quit.quit();
        glib::Propagation::Proceed
    });
    glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
        if let Ok(bytes) = std::fs::read(root.join("command.json")) {
            if let Ok(command) = serde_json::from_slice::<Value>(&bytes) {
                let id = command["sequence"].to_string();
                if id != *sequence.borrow() {
                    *sequence.borrow_mut() = id;
                    match command["action"].as_str().unwrap_or("") {
                        "capture" => {
                            let paintable = gtk::WidgetPaintable::new(Some(&window));
                            let snapshot = gtk::Snapshot::new();
                            paintable.snapshot(
                                &snapshot,
                                window.width() as f64,
                                window.height() as f64,
                            );
                            if let Some(node) = snapshot.to_node() {
                                if let Some(renderer) = window.renderer() {
                                    let texture = renderer.render_texture(&node, None);
                                    texture
                                        .save_to_png(
                                            root.join(
                                                command["file"].as_str().unwrap_or("render.png"),
                                            ),
                                        )
                                        .unwrap();
                                }
                            }
                        }
                        "update" => view.update(&command["state"]),
                        "error" => {
                            view.reject_pending(command["message"].as_str().unwrap_or("Rejected"))
                        }
                        "open" => view.open(
                            command["collection"].as_str().unwrap_or("todo"),
                            command["view"].as_str().unwrap_or("all"),
                        ),
                        "edit" => view.edit(command["task"].clone()),
                        "sort" => {
                            fn select(w: &gtk::Widget, index: u32) {
                                if w.widget_name() == "task-sort" {
                                    w.downcast_ref::<gtk::DropDown>()
                                        .unwrap()
                                        .set_selected(index);
                                    return;
                                }
                                let mut child = w.first_child();
                                while let Some(c) = child {
                                    select(&c, index);
                                    child = c.next_sibling();
                                }
                            }
                            select(
                                view.root.upcast_ref(),
                                command["index"].as_u64().unwrap_or(0) as u32,
                            );
                        }
                        "preferences" => {
                            view.configure_preferences(command["value"].clone(), |_| {})
                        }
                        _ => {}
                    }
                }
            }
        }
        fn walk(widget: &gtk::Widget, root: &gtk::Widget, rows: &mut Vec<Value>) {
            if widget.is_visible() {
                if let Some(b) = widget.compute_bounds(root) {
                    let text = widget
                        .downcast_ref::<gtk::Label>()
                        .map(|l| l.text().to_string())
                        .unwrap_or_default();
                    rows.push(json!({"instance":widget.as_ptr() as usize,"type":widget.type_().name(),"name":widget.widget_name().to_string(),"classes":widget.css_classes().iter().map(|s| s.to_string()).collect::<Vec<_>>(),"text":text,"x":b.x(),"y":b.y(),"width":b.width(),"height":b.height(),"tooltip":widget.tooltip_text().map(|s| s.to_string()),"focus":widget.has_focus()}));
                }
                let mut child = widget.first_child();
                while let Some(w) = child {
                    walk(&w, root, rows);
                    child = w.next_sibling();
                }
            }
        }
        let mut geometry = vec![];
        walk(view.root.upcast_ref(), window.upcast_ref(), &mut geometry);
        let value = json!({"state":view.state(),"requests":*requests.borrow(),"geometry":geometry});
        let _ = std::fs::write(
            root.join("inspect.tmp"),
            serde_json::to_vec(&value).unwrap(),
        );
        let _ = std::fs::rename(root.join("inspect.tmp"), root.join("inspect.json"));
        glib::ControlFlow::Continue
    });
    main_loop.run();
}
