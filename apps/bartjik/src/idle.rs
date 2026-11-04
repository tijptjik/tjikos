//! Idle policy overrides and a persistent, bar-independent keep-awake timer.
use crate::util::*;
use anyhow::{ensure, Context, Result};
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{io::Write, os::unix::fs::MetadataExt, path::PathBuf};

const KEYS: [&str; 4] = ["dim", "lock", "screenOff", "suspend"];
const DROPIN: &str = "zz-bartjik-idle.conf";
fn settings_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("bartjik/idle.json")
}
// Match Dotjik's existing helper and ExecCondition, including its shared lock.
fn preference() -> PathBuf {
    home().join(".local/state/idle-inhibit/state")
}
fn lease_path() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("bartjik/idle-timer.json")
}
fn settings() -> Result<Value> {
    let v = if settings_path().exists() {
        read(settings_path())?
    } else {
        json!({"duration":0,"policies":{}})
    };
    ensure!(
        [0, 1800, 3600, 7200].contains(&v["duration"].as_u64().unwrap_or(u64::MAX)),
        "Invalid keep-awake duration"
    );
    ensure!(v["policies"].is_object(), "Invalid saved idle settings");
    Ok(v)
}
fn saved_inhibition() -> Result<Option<bool>> {
    match std::fs::read_to_string(preference()) {
        Ok(value) => match value.trim() {
            "on" => Ok(Some(true)),
            "off" => Ok(Some(false)),
            _ => anyhow::bail!("Invalid saved keep-awake preference"),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn write_preference(on: bool) -> Result<()> {
    let path = preference();
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut f = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    writeln!(f, "{}", if on { "on" } else { "off" })?;
    f.as_file().sync_all()?;
    f.persist(path)?;
    Ok(())
}
fn fingerprint() -> Result<Value> {
    let p = preference();
    let m = p.metadata()?;
    Ok(json!([
        m.dev(),
        m.ino(),
        m.mtime(),
        m.mtime_nsec(),
        std::fs::read_to_string(p)?
    ]))
}
async fn running() -> Result<bool> {
    let uid = unsafe { libc::geteuid() }.to_string();
    succeeds(&["pgrep", "-u", &uid, "-x", "hypridle"]).await
}
async fn succeeds(args: &[&str]) -> Result<bool> {
    match cmd(args).await {
        Ok(_) => Ok(true),
        Err(e) if e.to_string().contains("exit 1") => Ok(false),
        Err(e) => Err(e),
    }
}
pub fn check() -> Result<()> {
    ensure!(saved_inhibition()? != Some(true), "Keep awake is enabled");
    Ok(())
}

#[derive(Debug)]
struct Source {
    path: PathBuf,
    program: String,
    flags: Vec<String>,
}
fn source_from_unit(unit: &str) -> Result<Source> {
    let mut skip = false;
    let mut service = false;
    let mut command = String::new();
    for line in unit.lines() {
        let line = line.trim();
        if line.starts_with("# /") {
            skip = line.ends_with(DROPIN);
            service = false;
        }
        if skip || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            service = line == "[Service]";
        }
        if service {
            if let Some(v) = line.strip_prefix("ExecStart=") {
                command = v.trim().to_owned();
            }
        }
    }
    // Accept Hypridle's documented flags, without evaluating shell commands.
    let words = Regex::new(r#""([^"]*)"|'([^']*)'|(\S+)"#).unwrap();
    let args: Vec<String> = words
        .captures_iter(&command)
        .map(|c| {
            c.get(1)
                .or_else(|| c.get(2))
                .or_else(|| c.get(3))
                .unwrap()
                .as_str()
                .to_owned()
        })
        .collect();
    let program = args
        .first()
        .context("Hypridle service has no startup command")?;
    ensure!(
        PathBuf::from(program)
            .file_name()
            .is_some_and(|v| v == "hypridle"),
        "Custom Hypridle launcher cannot be edited by this panel"
    );
    let mut path = xdg("XDG_CONFIG_HOME", ".config").join("hypr/hypridle.conf");
    let mut flags = vec![];
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-c" | "--config" => {
                i += 1;
                let raw = args.get(i).context("Missing Hypridle config path")?;
                path = expand(
                    &raw.replace("%h", &home().to_string_lossy())
                        .replace("%E", &xdg("XDG_CONFIG_HOME", ".config").to_string_lossy()),
                );
            }
            "-q" | "--quiet" | "-v" | "--verbose" => flags.push(args[i].clone()),
            _ => anyhow::bail!("Unsupported Hypridle startup option"),
        }
        i += 1;
    }
    ensure!(path.is_absolute(), "Hypridle config path must be absolute");
    Ok(Source {
        path,
        program: program.clone(),
        flags,
    })
}
async fn source() -> Result<Source> {
    source_from_unit(&cmd(&["systemctl", "--user", "cat", "hypridle.service"]).await?)
}
fn listeners() -> Regex {
    Regex::new(r"(?ms)^[ \t]*listener[ \t]*\{[ \t]*(?:#[^\n]*)?\n.*?^[ \t]*\}").unwrap()
}
fn field<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    block.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then_some(v.trim())
    })
}
fn kind(block: &str) -> Option<&'static str> {
    let command = field(block, "on-timeout")?;
    if command.contains("hyprsunset gamma ") {
        Some("dim")
    } else if command.starts_with("loginctl lock-session")
        || command.starts_with("hyprlock")
        || command.contains("/tailscale-lock")
    {
        Some("lock")
    } else if command.contains("dpms off") {
        Some("screenOff")
    } else if command == "systemctl suspend" {
        Some("suspend")
    } else {
        None
    }
}
fn parse_delays(text: &str) -> Result<Value> {
    ensure!(
        Regex::new(r"(?m)^[ \t]*listener[ \t]*\{")
            .unwrap()
            .find_iter(text)
            .count()
            == listeners().find_iter(text).count(),
        "This idle policy uses unsupported listener formatting"
    );
    ensure!(
        !text
            .lines()
            .any(|l| l.trim_start().starts_with("source") && l.contains('=')),
        "Edit sourced idle policies in their configuration files"
    );
    let mut result = json!({"dim":0,"lock":0,"screenOff":0,"suspend":0});
    let mut seen = std::collections::HashSet::new();
    for block in listeners().find_iter(text) {
        if let Some(key) = kind(block.as_str()) {
            ensure!(
                seen.insert(key),
                "Multiple {key} idle actions; edit their config directly"
            );
            let seconds = field(block.as_str(), "timeout")
                .context("Idle action has no timeout")?
                .split('#')
                .next()
                .unwrap()
                .trim()
                .parse::<u64>()?;
            result[key] = json!(seconds);
        }
    }
    Ok(result)
}
fn validate_delays(v: &Value) -> Result<()> {
    ensure!(
        v.is_object()
            && obj(v).len() == 4
            && KEYS
                .iter()
                .all(|k| v[k].as_u64().is_some_and(|n| n <= 86400)),
        "Idle delays must be between 0 (Never) and 86400 seconds"
    );
    Ok(())
}
struct Policy {
    source: Source,
    text: String,
    delays: Value,
    revision: String,
}
async fn policy(v: &Value) -> Result<Policy> {
    let source = source().await?;
    let text =
        std::fs::read_to_string(&source.path).context("Could not read the active idle policy")?;
    let mut delays = parse_delays(&text)?;
    let overrides = &v["policies"][source.path.to_string_lossy().as_ref()];
    for key in KEYS {
        if !overrides[key].is_null() {
            delays[key] = overrides[key].clone();
        }
    }
    validate_delays(&delays)?;
    let revision = format!(
        "{:x}",
        Sha256::digest(format!("{}\n{text}\n{overrides}", source.path.display()))
    );
    Ok(Policy {
        source,
        text,
        delays,
        revision,
    })
}
fn render(policy: &Policy, delays: &Value) -> Result<String> {
    validate_delays(delays)?;
    let mut seen = std::collections::HashSet::new();
    let timeout = Regex::new(r"(?m)^([ \t]*timeout[ \t]*=[ \t]*)\d+").unwrap();
    let mut output = listeners()
        .replace_all(&policy.text, |c: &regex::Captures| {
            let block = c.get(0).unwrap().as_str();
            let Some(key) = kind(block) else {
                return block.to_owned();
            };
            seen.insert(key);
            if delays[key] == 0 {
                return String::new();
            }
            timeout
                .replace(block, |c: &regex::Captures| {
                    format!("{}{}", &c[1], delays[key])
                })
                .into_owned()
        })
        .into_owned();
    // Missing actions use the normal policy's commands when present. Keep all
    // existing general hooks, keyboard listeners and custom commands verbatim.
    let normal =
        std::fs::read_to_string(xdg("XDG_CONFIG_HOME", ".config").join("hypr/hypridle.conf"))
            .unwrap_or_default();
    for key in KEYS {
        if seen.contains(key) || delays[key] == 0 {
            continue;
        }
        let native = listeners();
        let existing = native
            .find_iter(&normal)
            .find(|b| kind(b.as_str()) == Some(key));
        let default = match key {
            "dim" => "listener {\n    timeout = 60\n    on-timeout = hyprctl hyprsunset gamma 10\n    on-resume = hyprctl hyprsunset gamma 100\n}",
            "lock" => "listener {\n    timeout = 180\n    on-timeout = loginctl lock-session\n}",
            "screenOff" => "listener {\n    timeout = 600\n    on-timeout = hyprctl dispatch dpms off\n    on-resume = hyprctl dispatch dpms on\n}",
            _ => "listener {\n    timeout = 1800\n    on-timeout = systemctl suspend\n}",
        };
        output.push('\n');
        output.push_str(&timeout.replace(
            existing.map(|b| b.as_str()).unwrap_or(default),
            |c: &regex::Captures| format!("{}{}", &c[1], delays[key]),
        ));
        output.push('\n');
    }
    Ok(output)
}
pub async fn run() -> Result<()> {
    check()?;
    let v = settings()?;
    let source = source().await?;
    let path = if v["policies"][source.path.to_string_lossy().as_ref()].is_null() {
        source.path.clone()
    } else {
        let policy = policy(&v).await?;
        let path = xdg("XDG_RUNTIME_DIR", ".cache").join("bartjik-idle.conf");
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        file.write_all(render(&policy, &policy.delays)?.as_bytes())?;
        file.persist(&path)?;
        path
    };
    use std::os::unix::process::CommandExt;
    Err(std::process::Command::new(source.program)
        .args(source.flags)
        .arg("--config")
        .arg(path)
        .exec()
        .into())
}
async fn set_inhibited(on: bool, expected: Option<&Value>) -> Result<bool> {
    let p = preference();
    std::fs::create_dir_all(p.parent().unwrap())?;
    let _lock = crate::actions::FileLock::acquire(
        p.with_file_name("lock")
            .to_str()
            .context("Invalid preference path")?,
    )
    .await?;
    if expected.is_some_and(|e| fingerprint().ok().as_ref() != Some(e)) {
        return Ok(false);
    }
    let before = saved_inhibition()?.unwrap_or(!running().await?);
    write_preference(on)?;
    let result = async {
        cmd(&["systemctl", "--user", "stop", "hypridle.service"]).await?;
        let uid = unsafe { libc::geteuid() }.to_string();
        // Handle a legacy unmanaged process as the existing helper does.
        let _ = succeeds(&["pkill", "-u", &uid, "-x", "hypridle"]).await?;
        for _ in 0..60 {
            if !running().await? {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        ensure!(!running().await?, "Hypridle did not stop");
        if !on {
            cmd(&["systemctl", "--user", "start", "hypridle.service"]).await?;
            for _ in 0..60 {
                if running().await? {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            ensure!(
                running().await?,
                "Hypridle did not start; check the idle service"
            );
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if let Err(error) = result {
        write_preference(before)?;
        return Err(error);
    }
    Ok(true)
}
fn clear_lease() -> Result<()> {
    match std::fs::remove_file(lease_path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
fn lease() -> Option<Value> {
    let v = read(lease_path()).ok()?;
    (v["deadline"]
        .as_f64()
        .is_some_and(|d| d.is_finite() && d > 0.)
        && fingerprint().ok().as_ref() == Some(&v["preference"]))
    .then_some(v)
}
fn schedule(seconds: u64) -> Result<()> {
    if seconds == 0 {
        return clear_lease();
    }
    save(
        &lease_path(),
        &json!({"deadline":now()+seconds as f64,"preference":fingerprint()?,"error":""}),
    )
}
pub async fn tick() -> Result<()> {
    let _lock = crate::actions::FileLock::acquire("bartjik-idle.lock").await?;
    if !lease_path().exists() {
        return Ok(());
    }
    let Some(mut timer) = lease() else {
        return clear_lease();
    };
    if num(&timer["deadline"]) > now() {
        return Ok(());
    }
    match set_inhibited(false, Some(&timer["preference"])).await {
        Ok(_) => clear_lease(),
        Err(e) => {
            timer["preference"] = fingerprint()?;
            timer["error"] = json!(format!("Could not resume idle handling: {e}"));
            save(&lease_path(), &timer)?;
            Err(e)
        }
    }
}
pub async fn watch() -> Result<()> {
    loop {
        if let Err(e) = tick().await {
            eprintln!("Idle timer: {e}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}
pub async fn action(name: &str, selection: &str) -> Result<String> {
    let _lock = crate::actions::FileLock::acquire("bartjik-idle.lock").await?;
    match name {
        "lock" => {
            cmd(&["loginctl", "lock-session"]).await?;
            return Ok("Session locked".into());
        }
        "suspend" => {
            cmd(&["loginctl", "lock-session"]).await?;
            cmd(&["systemctl", "suspend"]).await?;
            return Ok("Sleep requested".into());
        }
        "poweroff" => {
            ensure!(selection == "confirm", "Confirm Power off now first");
            cmd(&["systemctl", "poweroff"]).await?;
            return Ok("Power off requested".into());
        }
        _ => {}
    }
    let mut v = settings()?;
    match name {
        "enabled" | "toggle" => {
            ensure!(
                name == "toggle" || ["on", "off"].contains(&selection),
                "Invalid keep-awake choice"
            );
            let on = if name == "toggle" {
                running().await?
            } else {
                selection == "on"
            };
            let duration = v["duration"].as_u64().unwrap();
            if on && duration > 0 {
                cmd(&["systemctl", "--user", "start", "bartjik-idle.service"])
                    .await
                    .context("Keep-awake timer service is unavailable; no changes applied")?;
            }
            let before = saved_inhibition()?.unwrap_or(!running().await?);
            set_inhibited(on, None).await?;
            if let Err(e) = schedule(if on { duration } else { 0 }) {
                let _ = set_inhibited(before, None).await;
                return Err(e);
            }
            Ok(if on {
                "Keep awake enabled"
            } else {
                "Automatic idle handling resumed"
            }
            .into())
        }
        "duration" => {
            let seconds = selection.parse::<u64>().context("Invalid duration")?;
            ensure!(
                [0, 1800, 3600, 7200].contains(&seconds),
                "Choose 30 minutes, 1 hour, 2 hours or until switched off"
            );
            let inhibited = saved_inhibition()?.unwrap_or(!running().await?);
            if inhibited && seconds > 0 {
                cmd(&["systemctl", "--user", "start", "bartjik-idle.service"])
                    .await
                    .context("Keep-awake timer service is unavailable; no changes applied")?;
            }
            if inhibited && saved_inhibition()?.is_none() {
                set_inhibited(true, None).await?;
            }
            if inhibited {
                schedule(seconds)?;
            }
            v["duration"] = json!(seconds);
            save(&settings_path(), &v)?;
            Ok("Keep-awake duration updated".into())
        }
        "delays" => {
            let request: Value = serde_json::from_str(selection)?;
            validate_delays(&request["delays"])?;
            let p = policy(&v).await?;
            ensure!(
                request["revision"] == p.revision,
                "Idle policy changed; reopen the panel before applying"
            );
            let _ = render(&p, &request["delays"])?;
            let previous = v.clone();
            v["policies"][p.source.path.to_string_lossy().as_ref()] = request["delays"].clone();
            save(&settings_path(), &v)?;
            if running().await? {
                if let Err(e) = cmd(&["systemctl", "--user", "restart", "hypridle.service"]).await {
                    save(&settings_path(), &previous)?;
                    let _ = cmd(&["systemctl", "--user", "restart", "hypridle.service"]).await;
                    return Err(
                        e.context("Could not apply idle delays; previous settings restored")
                    );
                }
            }
            Ok("Idle delays saved".into())
        }
        _ => anyhow::bail!("Unknown idle action"),
    }
}
fn row(info: Value) -> Value {
    let active = !truth(&info["running"]);
    let mut r = item(
        "idle",
        None,
        Some(if active { "coffee" } else { "hotel" }),
        "symbol",
        json!({"active":active}),
        if !s(&info["error"]).is_empty() {
            "Idle & Sleep · needs attention"
        } else if active {
            "Idle Inhibit : ON"
        } else {
            "Idle Inhibit : OFF"
        },
        json!({"leftClick":"caffeine.toggle","rightClick":"idle.panel","hoverEnter":"idle.panel"}),
    );
    r["panel"] = info;
    r
}
pub async fn snapshot() -> Result<Value> {
    let v = settings()?;
    let running = running().await?;
    let inhibited = saved_inhibition()?.unwrap_or(!running);
    let timer = lease();
    let mut info = json!({"title":"Idle & Sleep","rows":[],"inhibited":inhibited,"running":running,"duration":v["duration"],"deadline":timer.as_ref().map(|v|v["deadline"].clone()),"delays":{},"revision":"","editable":false,"gamePolicy":false,"error":if !inhibited && !running {"Idle service is not running"}else{""}});
    if let Some(timer) = timer {
        if !s(&timer["error"]).is_empty() {
            info["error"] = timer["error"].clone();
        }
    }
    match policy(&v).await {
        Ok(p) => {
            info["delays"] = p.delays;
            info["revision"] = json!(p.revision);
            info["editable"] = json!(true);
            info["gamePolicy"] = json!(p
                .source
                .path
                .file_name()
                .is_some_and(|n| n == "hypridle_game.conf"));
        }
        Err(e) => info["error"] = json!(e.to_string()),
    }
    Ok(row(info))
}
pub fn demo() -> Value {
    row(
        json!({"title":"Idle & Sleep","rows":[],"inhibited":true,"running":false,"duration":0,"deadline":null,"delays":{"dim":60,"lock":180,"screenOff":600,"suspend":1800},"revision":"synthetic","editable":true,"gamePolicy":false,"error":""}),
    )
}
