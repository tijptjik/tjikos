//! Native desktop bar. Provider policy remains in bartjik-core.
#[allow(dead_code)]
#[path = "../config.rs"]
mod config;
#[path = "../datetime.rs"]
mod datetime;
#[allow(dead_code)]
#[path = "../util.rs"]
mod util;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::Duration,
};
use tjik_desktop::{Desktop, DesktopState, WindowId};
use tjik_gtk::gtk::{self, gdk, glib, prelude::*};

#[path = "../action_choice.rs"]
mod action_choice;
mod audio_card;
mod audio_service;
mod clock_card;
mod clocks;
mod codex_card;
mod comparison;
mod connectivity_card;
mod crypto_chart;
mod edges;
mod history_card;
mod icons;
mod input;
mod layout;
mod left_card;
mod monitor_geometry;
mod motion;
#[allow(dead_code, unused_imports)]
#[path = "../network.rs"]
mod network;
mod network_card;
mod network_controller;
mod openrouter_card;
mod panel_reveal;
mod panel_view;
mod panels;
mod pointer;
#[path = "../profile.rs"]
mod profile;
mod recording;
mod runtime;
#[path = "../status_events.rs"]
mod status_events;
mod system_card;
mod thumbnail;
mod tray_menu;
mod tray_motion;
mod ui;
use ui::Ui;
#[derive(Clone)]
enum Action {
    PrepareToplevel(String, tjik_desktop::WorkspaceTarget, tjik_desktop::Rect),
    ReleaseToplevel(String),
    Window(WindowId),
    Close(WindowId),
    Workspace(tjik_desktop::WorkspaceTarget),
}
enum Event {
    CloseDetached(String),
    ToplevelPrepared(String, Result<(), String>),
    Hover(String, Option<(f64, f64)>),
    PanelCommand(String),
    ConnectivityAction(Value),
    SystemAction(String, Value),
    LeftRequest(Value),
    Backend(Value),
    Activity(String, Value),
    Input(Value),
    Quit,
    Desktop(DesktopState),
    Control(Value, std::sync::mpsc::SyncSender<Value>),
    Result(String),
    Comparison(Value),
}
fn socket() -> Result<PathBuf> {
    Ok(std::env::var_os("BARTJIK_GTK_SOCKET")
        .map(PathBuf::from)
        .unwrap_or(
            PathBuf::from(
                std::env::var_os("XDG_RUNTIME_DIR").context("Missing runtime directory")?,
            )
            .join("bartjik-gtk.sock"),
        ))
}
fn request(action: &str) -> Result<()> {
    let mut stream = UnixStream::connect(socket()?).context("GTK bar is not running")?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    writeln!(stream, "{}", json!({"action":action}))?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    let value: Value = serde_json::from_str(&line)?;
    println!("{value}");
    anyhow::ensure!(value["ok"] == true, "GTK request failed");
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let action = args.first().map(String::as_str).unwrap_or("serve");
    if ["status", "quit", "panel", "hide-panel", "reload"].contains(&action) {
        return request(action);
    }
    if action == "--help" {
        println!("bartjik [serve [--reserve] [--demo]|status|panel|hide-panel|reload|quit]\nGTK desktop bar. --reserve enables configured per-edge reservations.");
        return Ok(());
    }
    if action != "serve"
        || args
            .iter()
            .skip(1)
            .any(|a| !["--demo", "--reserve"].contains(&a.as_str()))
    {
        bail!("Unsupported GTK bar arguments");
    }
    let configuration = config::load()?;
    let demo = args.iter().any(|a| a == "--demo");
    let reserve = args.iter().any(|a| a == "--reserve");
    let desktop: Arc<dyn Desktop> = if demo {
        Arc::new(tjik_desktop::fake::Fake::default())
    } else {
        tjik_desktop::selected().clone()
    };
    anyhow::ensure!(
        desktop.diagnostics().capabilities.outputs,
        "Selected desktop cannot provide output state: {:?}",
        desktop.diagnostics().error
    );
    gtk::init().context("Cannot connect GTK to the selected display")?;
    anyhow::ensure!(
        gtk4_layer_shell::is_supported(),
        "GTK bar requires layer-shell"
    );
    let path = socket()?;
    if UnixStream::connect(&path).is_ok() {
        bail!("GTK bar already running");
    }
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _cleanup = Cleanup(path);
    let (tx, rx) = async_channel::bounded(8);
    if let Some(path) = std::env::var_os("BARTJIK_GTK_COMPARE_FILE") {
        let updates = tx.clone();
        std::thread::spawn(move || {
            // Directory monitoring survives the mirror's atomic file replacements.
            // All reads/parsing stay on this worker's private GLib context.
            let context = glib::MainContext::new();
            let result = context.with_thread_default(|| -> anyhow::Result<()> {
                let path = PathBuf::from(path);
                let directory = gio::File::for_path(path.parent().context("comparison parent")?);
                let monitor = directory.monitor_directory(
                    gio::FileMonitorFlags::WATCH_MOVES,
                    gio::Cancellable::NONE,
                )?;
                monitor.set_rate_limit(0);
                let previous = Rc::new(RefCell::new(Vec::new()));
                let read = Rc::new(move || {
                    use std::io::Read;
                    let mut data = Vec::new();
                    if let Ok(file) = std::fs::File::open(&path) {
                        if file.take(2_000_001).read_to_end(&mut data).is_ok()
                            && data.len() <= 2_000_000
                            && data != *previous.borrow()
                        {
                            if let Ok(value) = serde_json::from_slice(&data) {
                                if updates.send_blocking(Event::Comparison(value)).is_ok() {
                                    *previous.borrow_mut() = data;
                                }
                            }
                        }
                    }
                });
                let changed = read.clone();
                monitor.connect_changed(move |_, _, _, _| changed());
                read();
                glib::MainLoop::new(Some(&context), false).run();
                Ok(())
            });
            if !matches!(result, Ok(Ok(()))) {
                eprintln!("GTK comparison file monitor failed: {result:?}");
            }
        });
    }
    let runtime = if std::env::var_os("BARTJIK_GTK_COMPARE_FILE").is_none() {
        Some(runtime::Runtime::start(tx.clone(), demo, true))
    } else {
        None
    };
    let input_events = tx.clone();
    let controls = tx.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
            let mut line = String::new();
            if BufReader::new(std::io::Read::take(&mut stream, 4096))
                .read_line(&mut line)
                .is_err()
            {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if value["action"] == "quit" {
                let _ = writeln!(stream, "{}", json!({"ok":true}));
                let _ = controls.send_blocking(Event::Quit);
                break;
            }
            let (reply, read) = std::sync::mpsc::sync_channel(1);
            if controls
                .send_blocking(Event::Control(value, reply))
                .is_err()
            {
                break;
            }
            if let Ok(value) = read.recv_timeout(Duration::from_secs(2)) {
                let _ = writeln!(stream, "{value}");
            }
        }
    });
    let (actions, commands) = async_channel::bounded::<Action>(8);
    let actor = desktop.clone();
    let replies = tx.clone();
    std::thread::spawn(move || {
        while let Ok(id) = commands.recv_blocking() {
            if let Action::PrepareToplevel(title, workspace, rect) = &id {
                let result = if std::env::var_os("BARTJIK_GTK_COMPARE_FILE").is_some() {
                    Err("Read-only comparison".into())
                } else {
                    actor
                        .prepare_toplevel(title, workspace, *rect)
                        .map_err(|e| e.to_string())
                };
                let _ = replies.send_blocking(Event::ToplevelPrepared(title.clone(), result));
                continue;
            }
            let result = if std::env::var_os("BARTJIK_GTK_COMPARE_FILE").is_some() {
                Err(tjik_desktop::DesktopError::Unsupported)
            } else {
                match id {
                    Action::PrepareToplevel(..) => unreachable!(),
                    Action::ReleaseToplevel(title) => actor.release_toplevel(&title),
                    Action::Window(id) => actor.activate(&id),
                    Action::Close(id) => actor.close(&id),
                    Action::Workspace(target) => actor.select_workspace(&target),
                }
            };
            let message = result.err().map(|e| e.to_string()).unwrap_or_default();
            if replies.send_blocking(Event::Result(message)).is_err() {
                break;
            }
        }
    });
    let watcher = desktop.clone();
    std::thread::spawn(move || match watcher.subscribe() {
        Ok(events) => loop {
            match events.recv_timeout(Duration::from_secs(1)) {
                Ok(Some(event)) => {
                    if tx.send_blocking(Event::Desktop(event.state)).is_err() {
                        break;
                    }
                }
                Ok(None) => (),
                Err(e) => {
                    let _ = tx.send_blocking(Event::Desktop(DesktopState::Unavailable(e)));
                    break;
                }
            }
        },
        Err(e) => {
            let _ = tx.send_blocking(Event::Desktop(DesktopState::Unavailable(e)));
        }
    });
    let app = gtk::Application::builder()
        .application_id("org.tjikos.bartjik.gtk")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let receiver = RefCell::new(Some(rx));
    app.connect_activate(move |app| {
        if !gtk4_layer_shell::is_supported() {
            eprintln!("GTK bar requires layer-shell; refusing an unreserved window fallback");
            app.quit();
            return;
        }
        let Some(rx) = receiver.borrow_mut().take() else {
            return;
        };
        let style = &configuration["style"];
        let theme: tjik_gtk::theme::Theme =
            serde_json::from_value(style.clone()).unwrap_or_default();
        let css = tjik_gtk::theme::provider();
        tjik_gtk::theme::apply(
            &css,
            &theme,
            style["font"].as_str().unwrap_or("tijpset"),
            style["fontSize"].as_i64().unwrap_or(14) as i32,
        );
        let display = gdk::Display::default().unwrap();
        let ui = Rc::new(Ui::new(
            app.clone(),
            configuration.clone(),
            actions.clone(),
            demo,
            reserve,
            desktop.diagnostics().capabilities.workspace_select,
            desktop.clone(),
            runtime.clone(),
            input_events.clone(),
        ));
        let clock = Rc::downgrade(&ui);
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let Some(ui) = clock.upgrade() else {
                return glib::ControlFlow::Break;
            };
            ui.clock();
            glib::ControlFlow::Continue
        });
        let topology = Rc::downgrade(&ui);
        display.monitors().connect_items_changed(move |_, _, _, _| {
            if let Some(ui) = topology.upgrade() {
                if let Some(snapshot) = ui.snapshot.borrow().as_ref() {
                    ui.render(snapshot);
                }
            }
        });
        let hold = app.hold();
        glib::spawn_future_local(async move {
            let _hold = hold;
            while let Ok(event) = rx.recv().await {
                ui.event(event);
            }
        });
    });
    app.run_with_args::<&str>(&[]);
    Ok(())
}
