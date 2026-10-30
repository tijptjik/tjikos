use crate::{util::*, workspaces};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
const SNI: &str = "org.kde.StatusNotifierItem";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const PROPS: &str = "org.freedesktop.DBus.Properties";
pub async fn get(service: &str, path: &str, interface: &str, property: &str) -> Result<Value> {
    let v = jc(&[
        "busctl",
        "--user",
        "--json=short",
        "get-property",
        service,
        path,
        interface,
        property,
    ])
    .await?;
    Ok(v["data"].clone())
}
pub async fn all(service: &str, path: &str, interface: &str) -> Result<Value> {
    let v = jc(&[
        "busctl",
        "--user",
        "--json=short",
        "call",
        service,
        path,
        PROPS,
        "GetAll",
        "s",
        interface,
    ])
    .await?;
    Ok(json!(obj(&v["data"][0])
        .into_iter()
        .map(|(k, v)| (k, v["data"].clone()))
        .collect::<serde_json::Map<_, _>>()))
}
pub fn unread(id: &str, p: &Value) -> bool {
    if p["Status"] == "NeedsAttention" {
        return true;
    }
    if id == "TelegramDesktop" {
        return s(&p["IconName"]).to_lowercase().contains("attention");
    }
    if id != "ZapZap" {
        return false;
    }
    for pix in arr(&p["IconPixmap"]) {
        let (w, h) = (pix[0].as_u64().unwrap_or(0), pix[1].as_u64().unwrap_or(0));
        let pixels = arr(&pix[2]);
        if w == 0 || h == 0 || w.saturating_mul(h).saturating_mul(4) != pixels.len() as u64 {
            continue;
        }
        let red = pixels
            .chunks_exact(4)
            .filter(|p| {
                num(&p[0]) > 128. && num(&p[1]) > 180. && num(&p[2]) < 80. && num(&p[3]) < 80.
            })
            .count();
        if red as f64 >= 4f64.max((w * h) as f64 * 0.02) {
            return true;
        }
    }
    false
}
pub async fn roster() -> Vec<(String, String, Value)> {
    let v = get(
        WATCHER,
        "/StatusNotifierWatcher",
        WATCHER,
        "RegisteredStatusNotifierItems",
    )
    .await
    .unwrap_or(json!([]));
    let mut records = vec![];
    for name in arr(&v) {
        let (service, path) = s(name)
            .split_once('/')
            .map(|(s, p)| (s.to_owned(), format!("/{p}")))
            .unwrap_or((st(name), "/StatusNotifierItem".into()));
        if let Ok(props) = all(&service, &path, SNI).await {
            records.push((service, path, props));
        }
    }
    records
}
pub async fn attention() -> Result<()> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut monitor = tokio::process::Command::new("dbus-monitor")
        .args([
            "--session",
            "type='signal',interface='org.kde.StatusNotifierItem'",
            "type='signal',interface='org.kde.StatusNotifierWatcher'",
            "type='signal',interface='org.freedesktop.DBus.Properties'",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut lines = BufReader::new(monitor.stdout.take().unwrap()).lines();
    let mut previous = Value::Null;
    loop {
        let mut states = json!({});
        for (_, _, p) in roster().await {
            let id = s(&p["Id"]);
            if ["TelegramDesktop", "ZapZap"].contains(&id) {
                states[id] = json!(truth(&states[id]) || unread(id, &p));
            }
        }
        if states != previous {
            crate::emit(&states);
            previous = states;
        }
        match tokio::time::timeout(std::time::Duration::from_secs(15), lines.next_line()).await {
            Ok(Ok(None)) => anyhow::bail!("tray observer disconnected"),
            Ok(Err(e)) => return Err(e.into()),
            _ => {}
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        while let Ok(Ok(Some(_))) =
            tokio::time::timeout(std::time::Duration::from_millis(1), lines.next_line()).await
        {
        }
    }
}
pub fn select<'a>(app: &str, clients: &'a Value) -> Option<&'a Value> {
    let names: &[&str] = match app {
        "chatgpt" => &["chatgpt"],
        "telegram" => &[
            "org.telegram.desktop",
            "telegramdesktop",
            "telegram-desktop",
        ],
        "zapzap" => &["com.rtosta.zapzap", "zapzap"],
        "codex" => &["codex", "com.openai.codex"],
        "spotify" => &["spotify", "com.spotify.client"],
        _ => return None,
    };
    arr(clients)
        .iter()
        .filter(|c| {
            c["mapped"] != false
                && ["class", "initialClass"].iter().any(|k| {
                    let name = s(&c[k]).to_lowercase();
                    names.iter().any(|n| {
                        name == *n || (app == "telegram" && name.starts_with(&format!("{n}._")))
                    })
                })
        })
        .min_by_key(|c| c["focusHistoryID"].as_i64().unwrap_or(9999))
}
pub async fn focus(app: &str) -> Result<()> {
    ensure!(
        ["chatgpt", "telegram", "zapzap", "codex", "spotify"].contains(&app),
        "Unknown tray app"
    );
    let clients = workspaces::query("clients").await?;
    let mut client = select(app, &clients).cloned();
    if client.is_none() {
        ensure!(app != "spotify", "Spotify window is not mapped");
        let expected = if app == "telegram" {
            "telegramdesktop"
        } else {
            app
        };
        let mut activated = false;
        for (service, path, p) in roster().await {
            if [&p["Id"], &p["Title"], &p["ToolTip"][2]]
                .iter()
                .any(|v| s(v).to_lowercase() == expected)
            {
                cmd(&[
                    "busctl", "--user", "call", &service, &path, SNI, "Activate", "ii", "0", "0",
                ])
                .await?;
                activated = true;
                break;
            }
        }
        ensure!(activated, "Tray app is no longer registered");
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            client = select(app, &workspaces::query("clients").await?).cloned();
            if client.is_some() {
                break;
            }
        }
    }
    let c = client.context("Tray activation did not expose a window")?;
    let address = s(&c["address"]);
    ensure!(
        regex::Regex::new(r"^0x[0-9a-fA-F]+$")?.is_match(address),
        "Invalid compositor window address"
    );
    let monitors = workspaces::query("monitors").await?;
    let m = arr(&monitors)
        .iter()
        .find(|m| m["id"] == c["monitor"])
        .context("monitor unavailable")?;
    cmd(&[
        "hyprctl",
        "dispatch",
        &format!(
            "hl.dsp.focus({{ monitor = {} }})",
            workspaces::lua(s(&m["name"]))
        ),
    ])
    .await?;
    let w = s(&c["workspace"]["name"]);
    if let Some(special) = w.strip_prefix("special:") {
        if m["specialWorkspace"]["name"] != w {
            cmd(&[
                "hyprctl",
                "dispatch",
                &format!(
                    "hl.dsp.workspace.toggle_special({})",
                    workspaces::lua(special)
                ),
            ])
            .await?;
        }
    }
    cmd(&[
        "hyprctl",
        "dispatch",
        &format!(
            "hl.dsp.focus({{ window = {} }})",
            workspaces::lua(&format!("address:{address}"))
        ),
    ])
    .await?;
    for _ in 0..10 {
        if workspaces::query("activewindow").await?["address"] == address {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    anyhow::bail!("Compositor did not focus the requested app")
}
