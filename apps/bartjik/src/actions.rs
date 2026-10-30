use crate::util::*;
use anyhow::Result;
use serde_json::Value;
pub fn detach(args: &[String], context: &Value) -> Result<()> {
    use std::os::unix::process::CommandExt;
    std::process::Command::new(&args[0])
        .args(&args[1..])
        .env("BARTJIK_CONTEXT", context.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()?;
    Ok(())
}
use crate::{config, providers, workspaces};
use anyhow::ensure;
use serde_json::json;
pub fn choose(module: &Value, row: &Value, part: &str, event: &str) -> Value {
    let event = if event == "click" { "leftClick" } else { event };
    let mut result = Value::Null;
    for a in [
        &row["actions"],
        &row[part]["actions"],
        &module["actions"],
        &module[part]["actions"],
        &module["items"][s(&row["id"])]["actions"],
        &module["items"][s(&row["id"])][part]["actions"],
    ] {
        if event == "leftClick" {
            if let Some(v) = a.get("click") {
                result = v.clone();
            }
        }
        if let Some(v) = a.get(event) {
            result = v.clone();
        }
    }
    result
}
pub async fn route(name: &str) -> Result<()> {
    let sinks = jc(&["pactl", "-f", "json", "list", "sinks"]).await?;
    ensure!(
        !name.is_empty() && arr(&sinks).iter().any(|s| s["name"] == name),
        "The selected output was disconnected"
    );
    cmd(&["pactl", "set-default-sink", name]).await?;
    let streams = jc(&["pactl", "-f", "json", "list", "sink-inputs"]).await?;
    let mut failed = false;
    for stream in arr(&streams) {
        if cmd(&["pactl", "move-sink-input", &st(&stream["index"]), name])
            .await
            .is_err()
        {
            let remaining = jc(&["pactl", "-f", "json", "list", "sink-inputs"]).await?;
            if arr(&remaining)
                .iter()
                .any(|s| s["index"] == stream["index"])
            {
                failed = true;
            }
        }
    }
    ensure!(
        !failed,
        "Output changed; some playback streams could not be moved"
    );
    Ok(())
}
pub struct FileLock(std::fs::File);
impl FileLock {
    pub async fn acquire(name: &str) -> Result<Self> {
        use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
        let path = std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR")?).join(name);
        let f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        let start = now();
        loop {
            let result = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if result == 0 {
                return Ok(Self(f));
            }
            ensure!(now() - start < 30., "control busy");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
}
impl Drop for FileLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
pub async fn master(action: &str) -> Result<()> {
    let _lock = FileLock::acquire("audio-master.lock").await?;
    let (_, active) = providers::audio_state().await?;
    if action == "mute" {
        cmd(&[
            "pactl",
            "set-sink-mute",
            s(&active["name"]),
            if truth(&active["mute"]) { "0" } else { "1" },
        ])
        .await?;
    } else {
        let delta = action.parse::<i32>()?;
        let volume = (providers::volume(&active) + delta as f64).clamp(0., 100.);
        cmd(&[
            "pactl",
            "set-sink-volume",
            s(&active["name"]),
            &format!("{volume}%"),
        ])
        .await?;
    }
    Ok(())
}
pub async fn execute(
    action: &Value,
    c: &Value,
    m: &Value,
    row: &Value,
    items: &[Value],
) -> Result<()> {
    if action.is_null() {
        return Ok(());
    }
    if action.is_array() || action.is_object() {
        let args = config::argv(if action.is_object() {
            &action["command"]
        } else {
            action
        })?;
        if action["detach"] == true {
            detach(&args, c)?;
        } else {
            command(
                &args,
                m["actionTimeout"].as_f64().unwrap_or(15.),
                262144,
                &[("BARTJIK_CONTEXT".into(), c.to_string())],
                None,
            )
            .await?;
        }
        return Ok(());
    }
    let action = s(action);
    ensure!(
        arr(&config::contract()["INTERNAL"]).contains(&json!(action)),
        "unknown internal action"
    );
    if action.starts_with("workspace.") {
        let _lock = FileLock::acquire("bartjik-workspace.lock").await?;
        let mut target = row;
        let eligible: Vec<_> = items
            .iter()
            .filter(|r| !truth(&r["special"]) || truth(&m["scrollSpecial"]))
            .collect();
        if action != "workspace.activate" {
            if eligible.is_empty() {
                return Ok(());
            }
            let index = eligible
                .iter()
                .position(|r| truth(&r["state"]["active"]))
                .unwrap_or(0);
            target = eligible[if action.ends_with("next") {
                (index + 1) % eligible.len()
            } else {
                (index + eligible.len() - 1) % eligible.len()
            }];
        }
        let monitor = format!(
            "hl.dsp.focus({{ monitor = {} }})",
            workspaces::lua(s(&c["monitor"]))
        );
        let w = &target["workspace"];
        let target = if truth(&target["special"]) {
            format!(
                "hl.dsp.workspace.toggle_special({})",
                workspaces::lua(s(w).trim_start_matches("special:"))
            )
        } else {
            format!(
                "hl.dsp.focus({{ workspace = {} }})",
                if w.is_number() {
                    st(w)
                } else {
                    workspaces::lua(&format!("name:{}", s(w)))
                }
            )
        };
        cmd(&["hyprctl", "dispatch", &monitor]).await?;
        cmd(&["hyprctl", "dispatch", &target]).await?;
    } else if action.starts_with("audio.") && !action.ends_with(".panel") {
        match action {
            "audio.up" => master("4").await?,
            "audio.down" => master("-4").await?,
            "audio.mute" => master("mute").await?,
            "audio.route" => {
                let _lock = FileLock::acquire("audio-route.lock").await?;
                let (sinks, current) = providers::audio_state().await?;
                let choices = arr(&m["routes"]);
                let routes: Vec<_> = arr(&sinks)
                    .iter()
                    .filter(|r| {
                        if choices.is_empty() {
                            providers::headphones(r)
                                || s(&r["description"]).starts_with("Navi 21/23 HDMI/DP")
                        } else {
                            choices
                                .iter()
                                .any(|c| *c == r["name"] || *c == r["description"])
                        }
                    })
                    .collect();
                ensure!(
                    routes.len() >= 2,
                    "At least two available output routes are required"
                );
                let index = routes.iter().position(|r| r["name"] == current["name"]);
                route(s(
                    &routes[index.map(|i| (i + 1) % routes.len()).unwrap_or(0)]["name"]
                ))
                .await?;
            }
            _ => anyhow::bail!("unknown audio action"),
        }
    } else {
        match action {
            "locale.cycle" => {
                let _lock = FileLock::acquire("bartjik-locale.lock").await?;
                let current = cmd(&["fcitx5-remote", "--check", "-n"]).await?;
                let group = cmd(&["fcitx5-remote", "--check", "-q"]).await?;
                let methods = crate::details::methods(&group);
                ensure!(!methods.is_empty(), "Active Fcitx group is empty");
                let idx = methods
                    .iter()
                    .position(|m| *m == current)
                    .map(|i| (i + 1) % methods.len())
                    .unwrap_or(0);
                cmd(&["fcitx5-remote", "--check", "-s", &methods[idx]]).await?;
            }
            "DND.toggle" => {
                cmd(&[m["ctl"].as_str().unwrap_or("notjikctl"), "toggle"]).await?;
            }
            "tailscale.toggle" => {
                let _lock = FileLock::acquire("bartjik-tailscale.lock").await?;
                crate::connectivity::act("tailscale", "toggle", &Value::Null, m).await?;
            }
            "caffeine.toggle" | "nightlight.toggle" => {
                let name = if action.starts_with("caffeine") {
                    "hypridle"
                } else {
                    "hyprsunset"
                };
                let _lock = FileLock::acquire(&format!("bartjik-{name}.lock")).await?;
                if providers::running(name).await? {
                    cmd(&["pkill", "-x", name]).await?;
                } else {
                    detach(&[name.into()], &json!({}))?;
                }
            }
            "monitor.toggle" => {
                let path = m["commandPath"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        home()
                            .join(".local/bin/monitor-game-mode")
                            .to_string_lossy()
                            .into_owned()
                    });
                command(&[path, "toggle".into()], 15., 262144, &[], None).await?;
            }
            _ => anyhow::bail!("native action must be handled by QML"),
        }
    }
    Ok(())
}
#[derive(Default)]
pub struct Panels {
    panels: tokio::sync::Mutex<std::collections::HashMap<String, Vec<String>>>,
    closers: std::sync::Mutex<std::collections::HashMap<String, tokio::task::JoinHandle<()>>>,
}
impl Panels {
    pub async fn close(&self, except: Option<&str>) {
        let mut panels = self.panels.lock().await;
        let keys: Vec<_> = panels
            .keys()
            .filter(|k| Some(k.as_str()) != except)
            .cloned()
            .collect();
        for key in keys {
            if let Some(mut base) = panels.remove(&key) {
                base.push("close".into());
                let _ = command(&base, 2., 262144, &[], None).await;
            }
        }
    }
    pub async fn open(&self, kind: &str, c: &Value, m: &Value) -> Result<()> {
        if let Some(t) = self.closers.lock().unwrap().remove(kind) {
            t.abort();
        }
        self.close(Some(kind)).await;
        let mut panels = self.panels.lock().await;
        let hovering = c["event"] == "hoverEnter";
        if !hovering && panels.remove(kind).is_some() {
            return Ok(());
        }
        let path = m["panelPath"].as_str().map(expand).unwrap_or_else(|| {
            xdg("XDG_CONFIG_HOME", ".config")
                .join("quickshell")
                .join(kind)
        });
        ensure!(
            path.join("shell.qml").is_file(),
            "{kind} panel is not installed"
        );
        let anchor = &c["anchor"];
        let payload=json!({"screen":c["monitor"],"x":num(&anchor["x"])+num(&anchor["width"])/2.,"bottom":anchor["y"]}).to_string();
        let base = vec![
            "quickshell".into(),
            "ipc".into(),
            "-p".into(),
            path.to_string_lossy().into_owned(),
            "call".into(),
            kind.into(),
        ];
        let mut check = base.clone();
        check.push("isOpen".into());
        let opened = command(&check, 2., 262144, &[], None).await;
        if hovering && opened.as_ref().is_ok_and(|s| s == "true") {
            return Ok(());
        }
        let mut toggle = base.clone();
        toggle.extend(["toggleAt".into(), payload.clone()]);
        if opened.is_ok() && command(&toggle, 2., 262144, &[], None).await.is_ok() {
            if hovering {
                panels.insert(kind.into(), base);
            }
            return Ok(());
        }
        command(
            &[
                "quickshell".into(),
                "-d".into(),
                "-n".into(),
                "-p".into(),
                path.to_string_lossy().into_owned(),
            ],
            5.,
            262144,
            &[(format!("{}_WIDGET_ANCHOR", kind.to_uppercase()), payload)],
            None,
        )
        .await?;
        for _ in 0..30 {
            if command(&check, 1., 262144, &[], None)
                .await
                .is_ok_and(|v| v == "true")
            {
                if hovering {
                    panels.insert(kind.into(), base);
                }
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        anyhow::bail!("{kind} panel did not become ready")
    }
    pub fn leave(self: &std::sync::Arc<Self>, kind: &str) {
        let mut closers = self.closers.lock().unwrap();
        if let Some(t) = closers.remove(kind) {
            t.abort();
        }
        let this = self.clone();
        let name = kind.to_owned();
        closers.insert(
            kind.into(),
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                loop {
                    let base = this.panels.lock().await.get(&name).cloned();
                    let Some(mut base) = base else { return };
                    base.push("geometry".into());
                    let geometry = command(&base, 2., 262144, &[], None)
                        .await
                        .ok()
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                        .unwrap_or(Value::Null);
                    if truth(&geometry["dragging"]) {
                        this.panels.lock().await.remove(&name);
                        return;
                    }
                    if truth(&geometry["hovered"]) {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                        continue;
                    }
                    this.close(None).await;
                    return;
                }
            }),
        );
    }
    pub fn cancel(&self) {
        for (_, t) in self.closers.lock().unwrap().drain() {
            t.abort();
        }
    }
}
impl Drop for Panels {
    fn drop(&mut self) {
        for (_, t) in self.closers.get_mut().unwrap().drain() {
            t.abort();
        }
    }
}
