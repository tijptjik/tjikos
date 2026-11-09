//! Private-compositor integration driver. Never mutates a normal desktop session.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = std::env::var("XDG_RUNTIME_DIR")?;
    if !runtime.starts_with("/tmp/bc-") {
        return Err("Private fixture runtime required".into());
    }
    let desktop = tjik_desktop::selected();
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("watch") => {
            use std::io::Write;
            let events = desktop.subscribe()?;
            loop {
                if let Some(event) = events.recv_timeout(std::time::Duration::from_secs(1))? {
                    println!("{}", serde_json::to_string(&event)?);
                    std::io::stdout().flush()?;
                }
            }
        }
        Some("workspace") => {
            desktop.select_workspace(&serde_json::from_str::<tjik_desktop::WorkspaceTarget>(
                args.get(1).ok_or("Missing workspace intent")?,
            )?)?
        }
        Some("activate") => desktop.activate(&tjik_desktop::WindowId(
            args.get(1).ok_or("Missing target")?.clone(),
        ))?,
        Some("close") => desktop.close(&tjik_desktop::WindowId(
            args.get(1).ok_or("Missing target")?.clone(),
        ))?,
        _ => println!("{}", serde_json::to_string(&desktop.snapshot()?)?),
    }
    Ok(())
}
