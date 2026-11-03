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
    select_class(clients, names, app == "telegram")
}
fn select_class<'a>(clients: &'a Value, names: &[&str], telegram: bool) -> Option<&'a Value> {
    arr(clients)
        .iter()
        .filter(|c| {
            c["mapped"] != false
                && ["class", "initialClass"].iter().any(|k| {
                    let name = s(&c[k]).to_lowercase();
                    names.iter().any(|n| {
                        name.eq_ignore_ascii_case(n)
                            || (telegram && name.starts_with(&format!("{n}._")))
                    })
                })
        })
        .min_by_key(|c| c["focusHistoryID"].as_i64().unwrap_or(9999))
}

fn desktop_app(app: &str) -> Result<(std::path::PathBuf, String)> {
    let mut dirs = vec![xdg("XDG_DATA_HOME", ".local/share")];
    dirs.extend(std::env::split_paths(
        &std::env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
    ));
    let mut seen = std::collections::HashSet::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir.join("applications")) else {
            continue;
        };
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.extension().is_none_or(|e| e != "desktop")
                || !seen.insert(path.file_name().unwrap().to_os_string())
            {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let mut entry = std::collections::HashMap::new();
            let mut main = false;
            for line in text.lines().map(str::trim) {
                if line.starts_with('[') {
                    main = line == "[Desktop Entry]";
                } else if main && !line.starts_with('#') {
                    if let Some((key, value)) = line.split_once('=') {
                        entry.insert(key.trim(), value.trim());
                    }
                }
            }
            let name = entry.get("Name").copied().unwrap_or("");
            let expected = if app == "codex-lb" {
                "Codex LB"
            } else {
                "ChatGPT"
            };
            if entry.get("Type") == Some(&"Application")
                && entry.get("Hidden") != Some(&"true")
                && (name.eq_ignore_ascii_case(expected)
                    || path.file_stem().is_some_and(|id| id == app))
            {
                let class = entry
                    .get("StartupWMClass")
                    .copied()
                    .unwrap_or(app)
                    .to_owned();
                return Ok((path, class));
            }
        }
    }
    anyhow::bail!("{app} desktop app is not installed")
}

pub async fn open_app(app: &str) -> Result<()> {
    ensure!(["chatgpt", "codex-lb"].contains(&app), "Unknown app");
    let _lock = crate::actions::FileLock::acquire(&format!("bartjik-open-{app}.lock")).await?;
    let desktop = desktop_app(app);
    let class = desktop
        .as_ref()
        .map(|(_, class)| class.as_str())
        .unwrap_or(app);
    let names = [app, class];
    if let Some(client) = select_class(&workspaces::query("clients").await?, &names, false) {
        return focus_client(client).await;
    }
    let (path, _) = desktop
        .as_ref()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    // Give the app its own service so it survives subsequent bar restarts.
    cmd(&[
        "systemd-run",
        "--user",
        "--collect",
        "--quiet",
        "--property=ExitType=cgroup",
        "--",
        "gio",
        "launch",
        &path.to_string_lossy(),
    ])
    .await?;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if let Some(client) = select_class(&workspaces::query("clients").await?, &names, false) {
            return focus_client(client).await;
        }
    }
    anyhow::bail!("{app} did not expose a window after launch")
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
    focus_client(&c).await
}
async fn focus_client(c: &Value) -> Result<()> {
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
    let current_special = s(&m["specialWorkspace"]["name"]);
    if current_special != w {
        if let Some(special) = current_special.strip_prefix("special:") {
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
