use anyhow::Result;
use gtk::{glib, prelude::*};
use std::{cell::Cell, rc::Rc, time::Duration};
pub fn run() -> Result<()> {
    let seconds = std::env::var("RUNTJIK_COUNTDOWN_SECONDS")?.parse::<u32>()?;
    let success = std::env::var("RUNTJIK_COUNTDOWN_SUCCESS")?;
    let app = gtk::Application::builder()
        .application_id("org.tjikos.Runtjik.Countdown")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Runtjik countdown")
            .default_width(220)
            .default_height(200)
            .decorated(false)
            .build();
        window.add_css_class("tjik");
        let css = tjik_gtk::theme::provider();
        tjik_gtk::theme::apply(&css, &Default::default(), "monospace", 14);
        tjik_gtk::surface::launcher(&window, "runtjik-countdown");
        if let Ok(output) = std::env::var("RUNTJIK_COUNTDOWN_OUTPUT") {
            tjik_gtk::surface::monitor(&window, &output);
        }
        let remaining = Rc::new(Cell::new(seconds));
        let number = remaining.clone();
        let drawing = gtk::DrawingArea::new();
        drawing.set_draw_func(move |_, cr, width, height| {
            cr.select_font_face(
                "monospace",
                gtk::cairo::FontSlant::Normal,
                gtk::cairo::FontWeight::Bold,
            );
            cr.set_font_size(88.);
            let text = number.get().to_string();
            let Ok(extents) = cr.text_extents(&text) else {
                return;
            };
            cr.move_to(
                (width as f64 - extents.width()) / 2. - extents.x_bearing(),
                (height as f64 - extents.height()) / 2. - extents.y_bearing(),
            );
            cr.text_path(&text);
            cr.set_source_rgba(0.04, 0.03, 0.06, 0.9);
            cr.set_line_width(4.);
            let _ = cr.stroke_preserve();
            let gradient = gtk::cairo::LinearGradient::new(
                0.,
                height as f64 * 0.25,
                width as f64,
                height as f64 * 0.75,
            );
            for (at, r, g, b) in [
                (0., 1., 0.32, 0.65),
                (0.33, 0.65, 0.42, 1.),
                (0.66, 0.3, 0.77, 1.),
                (1., 0.55, 1., 0.75),
            ] {
                gradient.add_color_stop_rgb(at, r, g, b);
            }
            let _ = cr.set_source(&gradient);
            let _ = cr.fill();
        });
        window.set_child(Some(&drawing));
        let weak = app.downgrade();
        let file = success.clone();
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let n = remaining.get().saturating_sub(1);
            remaining.set(n);
            drawing.queue_draw();
            if n == 0 {
                if let Err(e) = std::fs::write(&file, []) {
                    eprintln!("Countdown: {e}");
                }
                if let Some(app) = weak.upgrade() {
                    app.quit();
                }
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = app.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                if let Some(app) = weak.upgrade() {
                    app.quit();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        window.add_controller(keys);
        window.present();
    });
    app.run_with_args(&["runtjik-countdown"]);
    Ok(())
}
