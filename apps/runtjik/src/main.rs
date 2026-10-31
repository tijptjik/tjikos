mod browser;
mod browser_host;
mod capture;
mod capture_worker;
mod clipboard;
mod close;
mod config;
mod controller;
mod discovery;
mod icons;
mod image_viewer;
mod launcher;
mod menus;
mod offline_history;
mod paths;
mod settings;
mod stats;
mod store;
mod todo;
mod trash;
mod util;
mod windows;
mod words;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
fn emit(v: &Value) -> Result<()> {
    let mut out = io::stdout().lock();
    writeln!(out, "{v}")?;
    out.flush()?;
    Ok(())
}
fn main() -> std::process::ExitCode {
    match execute() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            if std::env::args().nth(1).as_deref() == Some("browser-host") {
                eprintln!("Browser host: {e}");
            } else {
                let _ = emit(&json!({"kind":"error","message":e.to_string()}));
            }
            std::process::ExitCode::FAILURE
        }
    }
}
fn execute() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("serve");
    match cmd {
        "image" => crate::image_viewer::cli(&args[1..])?,
        "image-viewer" => crate::image_viewer::open(&serde_json::from_str(
            args.get(1)
                .ok_or_else(|| anyhow::anyhow!("Missing viewer specification"))?,
        )?)?,
        "capture-worker" => crate::capture_worker::serve(serde_json::from_str(
            args.get(1)
                .ok_or_else(|| anyhow::anyhow!("Missing capture specification"))?,
        )?)?,
        "register-browser-host" => crate::browser_host::register(&args[1..])?,
        "browser-host" => crate::browser_host::serve()?,
        "version" | "--version" => println!("0.8.3"),
        "check" => {
            config::load()?;
            println!("Configuration valid");
        }
        "stats" | "rebuild-counts" => {
            let store = store::Store::open()?;
            if cmd == "rebuild-counts" {
                store.rebuild()?;
            }
            emit(&store.counts()?)?;
        }
        "control" => controller::control(args.get(1).map(String::as_str).unwrap_or("toggle"))?,
        "migrate-config" => emit(&settings::migrate(
            args.iter().any(|v| v == "--enable-bundled-plugins"),
        )?)?,
        "serve" | "refresh" => {
            let mut app = launcher::Launcher::new(args.iter().any(|v| v == "--demo"))?;
            if cmd == "refresh" {
                app.refresh(true)?;
                emit(&json!({"indexed":app.entries.len()}))?;
            } else {
                emit(&app.initial()?)?;
                for line in io::stdin().lock().lines() {
                    let line = line?;
                    let request = serde_json::from_str::<Value>(&line);
                    let serial = request
                        .as_ref()
                        .ok()
                        .and_then(|v| v.get("serial"))
                        .cloned()
                        .unwrap_or(json!(0));
                    let response = match request {
                        Ok(v) => app.handle(v),
                        Err(e) => Err(e.into()),
                    };
                    match response {
                        Ok(Some(mut v)) => {
                            v["serial"] = serial;
                            emit(&v)?;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            emit(&json!({"kind":"error","message":e.to_string(),"serial":serial}))?
                        }
                    }
                }
            }
        }
        _ => anyhow::bail!("Unknown command: {cmd}"),
    }
    Ok(())
}
