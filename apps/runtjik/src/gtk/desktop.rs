//! GTK presentation mechanics consume normalized desktop models asynchronously.
use gtk::prelude::*;
use std::sync::OnceLock;
fn request<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(work());
    });
    gtk::glib::spawn_future_local(async move {
        if let Ok(value) = rx.recv().await {
            done(value);
        }
    });
}
pub fn place(window: &gtk::ApplicationWindow, requested: &str) {
    let weak = window.downgrade();
    let requested = requested.to_owned();
    request(
        move || tjik_desktop::selected().snapshot(),
        move |result| {
            let (Some(window), Ok(snapshot)) = (weak.upgrade(), result) else {
                return;
            };
            if let Some(output) = snapshot.outputs.iter().find(|o| {
                if requested == "focused" {
                    o.focused
                } else {
                    o.name == requested
                }
            }) {
                tjik_gtk::surface::monitor(&window, &output.name);
            }
        },
    );
}
pub fn watch(pinned: bool, demo: bool) {
    let Ok(exe) = std::env::var_os("RUNTJIK_HOME")
        .map(|h| std::path::PathBuf::from(h).join("runtjik-gtk"))
        .map(Ok)
        .unwrap_or_else(std::env::current_exe)
    else {
        return;
    };
    let command = format!(
        "{} outside-click{}",
        shlex::try_quote(&exe.to_string_lossy()).unwrap(),
        if demo { " --demo" } else { "" }
    );
    std::thread::spawn(move || {
        let desktop = tjik_desktop::selected();
        if desktop.diagnostics().capabilities.outside_click {
            if let Err(e) =
                desktop.observe_outside_click(std::process::id(), "runtjik-gtk", pinned, &command)
            {
                eprintln!("Outside-click observer: {e}");
            }
        }
    });
}
pub fn transfer(window: &gtk::ApplicationWindow, grab: (f64, f64)) {
    if std::env::var_os("TJIK_GTK_WINDOWED").is_some() {
        return;
    }
    let weak = window.downgrade();
    request(
        || {
            let d = tjik_desktop::selected();
            Ok::<_, tjik_desktop::DesktopError>((d.pointer()?, d.snapshot()?))
        },
        move |result| {
            let (Some(window), Ok(((x, y), snapshot))) = (weak.upgrade(), result) else {
                return;
            };
            for output in snapshot.outputs {
                let r = output.logical;
                if x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height {
                    tjik_gtk::surface::monitor(&window, &output.name);
                    let left =
                        (x - r.x - grab.0).clamp(0., (r.width - window.width() as f64).max(0.));
                    let top =
                        (y - r.y - grab.1).clamp(0., (r.height - window.height() as f64).max(0.));
                    tjik_gtk::surface::offset(
                        &window,
                        (left - (r.width - window.width() as f64) / 2.) as i32,
                        (top - (r.height - window.height() as f64) / 2.) as i32,
                    );
                    break;
                }
            }
        },
    );
}
/// One worker and a bounded queue; intermediate pointer updates may be coalesced.
pub fn pointer(done: Box<dyn FnOnce(Option<(f64, f64)>)>) {
    type Reply = async_channel::Sender<Option<(f64, f64)>>;
    static REQUESTS: OnceLock<async_channel::Sender<Reply>> = OnceLock::new();
    let requests = REQUESTS.get_or_init(|| {
        let (tx, rx) = async_channel::bounded::<Reply>(1);
        std::thread::spawn(move || {
            while let Ok(reply) = rx.recv_blocking() {
                let _ = reply.send_blocking(tjik_desktop::selected().pointer().ok());
            }
        });
        tx
    });
    let (tx, rx) = async_channel::bounded(1);
    if requests.try_send(tx).is_err() {
        return;
    }
    gtk::glib::spawn_future_local(async move {
        if let Ok(point) = rx.recv().await {
            done(point);
        }
    });
}

/// The adapter coalesces state for slow consumers; no per-window subscription.
pub fn events(send: async_channel::Sender<crate::Event>) {
    std::thread::spawn(move || {
        let Ok(subscription) = tjik_desktop::selected().subscribe() else {
            return;
        };
        while !send.is_closed() {
            match subscription.recv_timeout(std::time::Duration::from_millis(250)) {
                Ok(Some(event)) => {
                    if send.send_blocking(crate::Event::Desktop(event)).is_err() {
                        break;
                    }
                }
                Ok(None) => (),
                Err(_) => break,
            }
        }
    });
}
