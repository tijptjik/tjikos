mod auth;
mod auth_code;
mod background;
mod codex;
mod contacts;
mod google;
mod html;
mod labels;
mod model;
mod notifications;
mod push;
mod quotes;
mod rules;
mod store;
mod ui;
mod worker;

use anyhow::{Context, Result, bail};
use gpui_kit::*;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--sync"] || args == ["--sync-once"] {
        return background::run(store::data_dir()?, args == ["--sync-once"]);
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "gmailtjik — a native Gmail client\n\n  gmailtjik\n  gmailtjik --demo\n  gmailtjik --sync (headless background polling)\n  gmailtjik --sync-once\n  gmailtjik --connect /private/path/google-desktop-client.json [--contacts]\n\nPrivate state: $GMAILTJIK_DATA_DIR or $XDG_DATA_HOME/gmailtjik\nEnable Gmail API, and optionally People API, in your Google Cloud project."
        );
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--connect") {
        let path = args
            .get(1)
            .context("--connect requires a private Desktop OAuth client JSON path")?;
        return auth::connect(
            &store::data_dir()?,
            std::path::Path::new(path),
            args.iter().any(|a| a == "--contacts"),
        );
    }
    let demo = args == ["--demo"];
    if !args.is_empty() && !demo {
        bail!("Unknown argument. Use gmailtjik --help");
    }
    let dir = if demo { None } else { Some(store::data_dir()?) };
    // Validate the cache before entering the UI event loop.
    let _ = store::Store::open(dir.as_deref())?;
    application().with_assets(assets::Assets).run(move |cx| {
        init(cx);
        ui::theme(cx);
        let options = WindowOptions {
            app_id: Some("gmailtjik".into()),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1360.), px(900.)),
                cx,
            ))),
            window_min_size: Some(size(px(1040.), px(700.))),
            titlebar: Some(TitlebarOptions {
                title: Some("Gmailtjik".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        open_window(options, cx, move |window, cx| {
            cx.new(|cx| ui::MailApp::new(demo, dir, window, cx))
        })
        .expect("Could not open Gmailtjik window");
        cx.activate(true);
    });
    Ok(())
}
