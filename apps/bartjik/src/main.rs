mod actions;
mod audio;
mod battery;
mod cache;
mod config;
mod connectivity;
mod demo;
mod details;
mod engine;
mod lyrics;
mod navigation;
mod network;
mod providers;
mod spotify;
mod tray;
mod util;
mod workspaces;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use util::*;
pub fn emit(v: &Value) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}
async fn stdin_json(limit: u64) -> Result<Value> {
    use tokio::io::AsyncReadExt;
    let mut raw = vec![];
    tokio::io::stdin()
        .take(limit + 1)
        .read_to_end(&mut raw)
        .await?;
    ensure!(raw.len() as u64 <= limit, "Input limit exceeded");
    Ok(serde_json::from_slice(&raw)?)
}
async fn ipc(request: &Value) -> Result<Value> {
    let qml = std::env::var("BARTJIK_QML")
        .unwrap_or_else(|_| root().join("qml").to_string_lossy().into_owned());
    let raw = cmd(&[
        "quickshell",
        "ipc",
        "--path",
        &qml,
        "call",
        "bartjik",
        "request",
        &request.to_string(),
    ])
    .await
    .context("bartjik is not running for this QML path")?;
    let value: Value = serde_json::from_str(&raw).context("Invalid bartjik IPC response")?;
    ensure!(
        value["ok"] == true,
        "{}",
        value["error"].as_str().unwrap_or("IPC failed")
    );
    Ok(value)
}
async fn run() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--config") {
        let path = std::path::absolute(args.get(i + 1).context("--config requires a path")?)?;
        std::env::set_var("BARTJIK_CONFIG", path);
        args.drain(i..=i + 1);
    }
    let op = args.first().map(String::as_str).unwrap_or("--help");
    match op{
 "--help"|"-h"=>println!("bartjik [--config PATH] [--demo] [--daemon]\nbartjikctl [--config PATH] <status|monitors|bars|modules|reload|config --validate|show-workspaces|hide-workspaces|show|hide|toggle|toggle-reveal|show-reveal|hide-reveal|init|spotify>"),
 "launch"=>{use std::os::unix::process::CommandExt;for a in &args[1..]{ensure!(["--demo","--daemon","--help","-h"].contains(&a.as_str()),"unknown launch option");}if args.iter().any(|a|a=="--help"||a=="-h"){println!("bartjik [--config PATH] [--demo] [--daemon]");return Ok(());}if args.iter().any(|a|a=="--demo"){std::env::set_var("BARTJIK_DEMO","1");}config::load()?;let qml=std::env::var("BARTJIK_QML").unwrap_or_else(|_|root().join("qml").to_string_lossy().into_owned());let mut cmd=std::process::Command::new("quickshell");cmd.args(["--path",&qml,"--no-duplicate"]);if args.iter().any(|a|a=="--daemon"){cmd.arg("--daemonize");}return Err(cmd.exec().into());},
 "serve"=>engine::serve(args.iter().any(|a|a=="--demo")||std::env::var("BARTJIK_DEMO").as_deref()==Ok("1")).await?,
 "check-assets"=>{config::check_assets()?;println!("bartjik: configurations, themes and font provenance checked");},
 "spotify"=>emit(&spotify::configure(args.get(1).context("spotify requires login, logout or status")?)?),
 "config" if args.get(1).is_some_and(|a|a=="--validate")=>emit(&json!({"ok":true,"config":config::load()?})),
 "init"=>{let option=|name:&str,default:&str|->String{args.iter().position(|a|a==name).and_then(|i|args.get(i+1)).cloned().unwrap_or(default.into())};let preset=option("--preset","minimal");let theme=option("--theme","monochrome");ensure!(["desktop","laptop","minimal"].contains(&preset.as_str())&&["monochrome","rose-punk"].contains(&theme.as_str()),"invalid preset or theme");let p=config::path();let mut v=read(root().join("examples").join(format!("{preset}.json")))?;v["theme"]=json!(theme);std::fs::create_dir_all(p.parent().unwrap())?;use std::io::Write;let mut f=std::fs::OpenOptions::new().create_new(true).write(true).open(&p).context("Config already exists; not overwriting it")?;writeln!(f,"{}",serde_json::to_string_pretty(&v)?)?;emit(&json!({"ok":true,"path":p}));},
 "helper"=>{let name=args.get(1).context("helper requires a name")?;let rest=&args[2..];match name.as_str(){"network"=>{if rest.first().is_some_and(|s|s=="traffic"){network::watch_traffic().await?;}else{let r=network::execute(&stdin_json(65536).await?).await.unwrap_or_else(|e|json!({"ok":false,"error":e.to_string()}));emit(&r);}},"audio"=>audio::controls(rest).await?,"audio-levels"=>audio::levels(!rest.iter().any(|a|a=="--playback-only")).await?,"tray-attention"=>tray::attention().await?,"tray-focus"=>tray::focus(rest.first().context("missing app")?).await?,"media-play"=>navigation::play().await?,"media-navigate"=>navigation::navigate(&stdin_json(4096).await?).await?,_=>anyhow::bail!("unknown helper")}},
 "status"|"monitors"|"bars"|"modules"|"reload"|"config"|"show-workspaces"|"hide-workspaces"=>{ensure!(args.len()==1,"unexpected arguments");emit(&ipc(&json!({"op":op})).await?);},
 "show"|"hide"|"toggle"|"toggle-reveal"|"show-reveal"|"hide-reveal"=>{ensure!(args.len()<=2,"unexpected arguments");emit(&ipc(&json!({"op":op,"bar":args.get(1).map(String::as_str).unwrap_or("all")})).await?);},
 _=>anyhow::bail!("unknown command: {op}")};
    Ok(())
}
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("signal handler");
    let result =
        tokio::select! {r=run()=>r,_=term.recv()=>Ok(()),_=tokio::signal::ctrl_c()=>Ok(())};
    if let Err(e) = result {
        eprintln!("{}", json!({"ok":false,"error":e.to_string()}));
        std::process::exit(1);
    }
}
#[cfg(test)]
mod contracts;
