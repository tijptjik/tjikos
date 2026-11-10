//! Standalone host. Task presentation and interaction are shared with Runtjik.
use serde_json::{json, Value};
use tjik_gtk::gtk::{self, gio, glib, prelude::*};

fn main() -> glib::ExitCode {
    let demo = std::env::args().any(|a| a == "--demo");
    let collection = std::env::var("TASKTJIK_COLLECTION").unwrap_or_else(|_| "todo".into());
    let app = gtk::Application::builder()
        .application_id(if demo {
            "hk.type.tasktjik.demo"
        } else {
            "hk.type.tasktjik"
        })
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let current = std::rc::Rc::new(std::cell::RefCell::new(
        None::<std::rc::Weak<tasktjik_gtk::TaskView>>,
    ));
    let active = current.clone();
    app.connect_command_line(move |app, command| {
        app.activate();
        let args: Vec<String> = command
            .arguments()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let value = |flag: &str| {
            args.windows(2)
                .find(|a| a[0] == flag)
                .map(|a| a[1].as_str())
        };
        if let Some(view) = active.borrow().as_ref().and_then(|v| v.upgrade()) {
            view.open(
                value("--collection").unwrap_or("todo"),
                value("--view").unwrap_or("all"),
            );
        }
        0.into()
    });
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Tasktjik")
            .default_width(860)
            .default_height(680)
            .build();
        window.add_css_class("tjik");
        let css = tjik_gtk::theme::provider();
        tjik_gtk::theme::apply_with_css(
            &css,
            &tjik_gtk::theme::Theme::default(),
            "sans-serif",
            14,
            tasktjik_gtk::style::CSS,
        );
        let (send, receive) = async_channel::bounded::<Value>(32);
        let (results, responses) = async_channel::bounded(32);
        std::thread::spawn(move || {
            let mut backend = tasktjik_api::cache::CachedTodo::default();
            while let Ok(request) = receive.recv_blocking() {
                let request_id = request["request_id"].as_str().unwrap_or("").to_owned();
                let result = backend
                    .state(request, &json!({}), demo)
                    .map_err(|e| format!("{e:#}"));
                if results.send_blocking((request_id, result)).is_err() {
                    break;
                }
            }
        });
        let commands = send.clone();
        let close = window.downgrade();
        let view = tasktjik_gtk::TaskView::new(
            move |value| {
                let _ = commands.try_send(value);
            },
            move || {
                if let Some(window) = close.upgrade() {
                    window.close();
                }
            },
            if demo {
                None
            } else {
                std::env::var_os("TASKTJIK_HOME").map(|home| {
                    tasktjik_gtk::preview_helper::provider(
                        std::path::PathBuf::from(home).join("qml/page-preview"),
                    )
                })
            },
        );
        view.configure_mutations(tasktjik_api::optimistic::project);
        *current.borrow_mut() = Some(std::rc::Rc::downgrade(&view));
        if !demo {
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| {
                    std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                        .join(".config")
                });
            let path = config.join("tasktjik/ui.ini");
            let weak = std::rc::Rc::downgrade(&view);
            view.configure_preferences(tasktjik_gtk::preferences::load(&path), move |value| {
                if let Err(message) = tasktjik_gtk::preferences::save(&path, value) {
                    if let Some(view) = weak.upgrade() {
                        view.error(&message);
                    }
                }
            });
        }
        if !demo {
            let state = std::env::var_os("XDG_STATE_HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| {
                    std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                        .join(".local/state")
                });
            if let Err(message) =
                view.configure_outbox(state.join("tasktjik/standalone-outbox.json"))
            {
                view.error(&message);
            }
        }
        view.update(&json!({"collection":collection}));
        window.set_child(Some(&view.root));
        let weak = std::rc::Rc::downgrade(&view);
        glib::MainContext::default().spawn_local(async move {
            while let Ok((request_id, result)) = responses.recv().await {
                let Some(view) = weak.upgrade() else { break };
                view.response(&request_id, result);
            }
        });
        let weak = std::rc::Rc::downgrade(&view);
        glib::timeout_add_seconds_local(10, move || {
            if let Some(view) = weak.upgrade() {
                view.refresh();
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        // Keep the application view alive exactly as long as its native window.
        let owned = view.clone();
        window.connect_destroy(move |_| {
            let _ = &owned;
            send.close();
        });
        view.refresh();
        window.present();
        view.focus();
    });
    let args = vec![
        "tasktjik-gtk".to_owned(),
        "--collection".into(),
        std::env::var("TASKTJIK_COLLECTION").unwrap_or_else(|_| "todo".into()),
        "--view".into(),
        std::env::var("TASKTJIK_VIEW").unwrap_or_else(|_| "all".into()),
    ];
    app.run_with_args(&args)
}
