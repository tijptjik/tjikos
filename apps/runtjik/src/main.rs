mod compat;
mod config;
mod controller;
mod discovery;
mod icons;
mod launcher;
mod store;
mod util;
mod windows;
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
            let _ = emit(&json!({"kind":"error","message":e.to_string()}));
            std::process::ExitCode::FAILURE
        }
    }
}
fn execute() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("serve");
    match cmd {
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
        "migrate-config" => compat::cli(&args)?,
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
