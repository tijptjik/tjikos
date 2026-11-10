//! Native GTK frontend. The GTK3 icon resolver stays in the existing core process.
mod app;
mod completion;
mod control;
mod countdown;
mod desktop;
mod previews;
mod recovery;
mod result_adapter;
mod style;
mod views;
mod worker;

use anyhow::Result;
use serde_json::Value;

pub enum Event {
    Response(u64, Value),
    Request(Value),
    ExitView,
    Quit,
    Desktop(tjik_desktop::DesktopEvent),
    Control(Value, std::sync::mpsc::SyncSender<Value>),
}

fn execute() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let demo = args.iter().any(|s| s == "--demo");
    let action = args.first().map(String::as_str).unwrap_or("toggle");
    if action == "countdown" {
        countdown::run()
    } else if action == "serve" {
        recovery::run(demo)
    } else if action == "surface" {
        app::run(demo)
    } else {
        control::run(action, &args[1.min(args.len())..], demo)
    }
}
fn main() -> std::process::ExitCode {
    match execute() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Runtjik GTK: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
