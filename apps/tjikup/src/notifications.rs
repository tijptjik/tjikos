use crate::{actions::State, scheduler::Receipt};
use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::{IsTerminal, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    label: String,
    value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tone: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Data {
    state: String,
    duration: String,
    tone: String,
    #[serde(default, rename = "rainbow-text")]
    rainbow_text: bool,
    results: Vec<Row>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    title: String,
    body: String,
    data: Data,
}

fn tone(value: &str) -> bool {
    ["success", "warning", "critical"].contains(&value)
}
fn send(argv: &[String]) -> bool {
    let Ok(mut child) = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            _ => (),
        }
        if start.elapsed() > Duration::from_secs(3) {
            let _ = child.kill();
            let _ = child.wait();
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn query(args: &[&str]) -> Option<Vec<u8>> {
    let output = tempfile::tempfile().ok()?;
    let mut child = Command::new("hyprctl")
        .args(args)
        .stdin(Stdio::null())
        .stdout(output.try_clone().ok()?)
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                use std::io::{Read, Seek};
                let mut output = output;
                output.rewind().ok()?;
                let mut bytes = Vec::new();
                output.take(1_048_576).read_to_end(&mut bytes).ok()?;
                return Some(bytes);
            }
            Ok(None) if start.elapsed() < Duration::from_secs(1) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

pub fn focus() -> Option<Vec<String>> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none()
        || std::env::var("TJIKUP_NOTIFY").as_deref() == Ok("0")
        || std::env::var_os("SSH_CONNECTION").is_some()
    {
        return None;
    }
    let clients: Vec<serde_json::Value> =
        serde_json::from_slice(&query(&["-j", "clients"])?).ok()?;
    let mut pid = std::process::id();
    let mut seen = std::collections::BTreeSet::new();
    while pid > 1 && seen.insert(pid) {
        let matches: Vec<_> = clients
            .iter()
            .filter(|c| c["pid"].as_u64() == Some(pid as u64))
            .collect();
        if matches.len() == 1 {
            let address = matches[0]["address"].as_str()?;
            if address
                .strip_prefix("0x")
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_hexdigit()))
            {
                if query(&["eval", "assert(hl.dsp.focus)"])
                    .is_some_and(|s| s == b"ok\n" || s == b"ok")
                {
                    return Some(vec![
                        "hyprctl".into(),
                        "dispatch".into(),
                        format!("hl.dsp.focus({{ window = \"address:{address}\" }})"),
                    ]);
                }
                return Some(vec![
                    "hyprctl".into(),
                    "dispatch".into(),
                    "focuswindow".into(),
                    format!("address:{address}"),
                ]);
            }
        }
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        pid = stat
            .rsplit_once(')')?
            .1
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()?;
    }
    None
}

pub fn native(card: &Card, focus: Option<&Vec<String>>) -> Result<bool> {
    if card.title.len() > 1024
        || card.body.len() > 8192
        || card.data.state.len() > 8192
        || card.data.duration.len() > 200
        || !tone(&card.data.tone)
        || card.data.results.len() != 2
        || card.data.results.iter().any(|r| {
            r.label.len() > 200 || r.value.len() > 200 || r.tone.as_ref().is_some_and(|t| !tone(t))
        })
    {
        bail!("invalid notification display metadata");
    }
    // Only display metadata crosses the protocol. Never accept remote action commands.
    let argv = vec![
        "notjikctl".into(),
        "send".into(),
        card.title.clone(),
        card.body.clone(),
        "--app".into(),
        "Tjikup".into(),
        "--icon".into(),
        "tjikup-notification".into(),
        "--type".into(),
        "results-table".into(),
        "--data".into(),
        serde_json::to_string(&card.data)?,
        "--actions".into(),
        serde_json::to_string(
            &focus
                .map(|command| vec![json!({"label":"Show window","command":command})])
                .unwrap_or_default(),
        )?,
        "--urgency".into(),
        if card.data.tone == "critical" {
            "critical"
        } else {
            "normal"
        }
        .into(),
        "--timeout".into(),
        if card.data.tone == "success" {
            "10s"
        } else {
            "until-dismissed"
        }
        .into(),
    ];
    Ok(send(&argv))
}

pub fn completion(receipt: &Receipt, focus: Option<&Vec<String>>) {
    if std::env::var("TJIKUP_NOTIFY").as_deref() == Ok("0") {
        return;
    }
    let failed: Vec<_> = receipt
        .recipes
        .iter()
        .filter(|(_, o)| o.state == State::Failed)
        .map(|(id, _)| id.clone())
        .collect();
    let stopped = receipt.recipes.values().any(|o| o.state == State::Stopped);
    let warnings: Vec<_> = receipt
        .recipes
        .values()
        .flat_map(|o| o.warnings.iter().cloned())
        .collect();
    let done = receipt
        .recipes
        .values()
        .filter(|o| matches!(o.state, State::Done | State::Unchanged))
        .count();
    let state = if receipt.interrupted {
        "INTERRUPTED"
    } else if !failed.is_empty() {
        "FAILED"
    } else if stopped || !warnings.is_empty() {
        "ATTENTION"
    } else {
        "ALL SYSTEMS GO"
    };
    let card = Card {
        title: format!("Tjikup · {}", receipt.title),
        body: failed
            .iter()
            .take(3)
            .map(|id| format!("Failed · {id}"))
            .chain(
                warnings
                    .iter()
                    .map(|subject| format!("Warning · {subject}")),
            )
            .take(3)
            .collect::<Vec<_>>()
            .join("\n"),
        data: Data {
            state: state.into(),
            rainbow_text: state == "ALL SYSTEMS GO",
            duration: format!("{:.0}s", receipt.seconds),
            tone: if receipt.exit_code() != 0 {
                "critical"
            } else if stopped || !warnings.is_empty() {
                "warning"
            } else {
                "success"
            }
            .into(),
            results: vec![
                Row {
                    label: "Stages completed".into(),
                    value: done.to_string(),
                    tone: None,
                },
                Row {
                    label: "Issues".into(),
                    value: (failed.len() + warnings.len() + usize::from(stopped)).to_string(),
                    tone: None,
                },
            ],
        },
    };
    if std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some() {
        if std::env::var("TERM").as_deref() != Ok("xterm-kitty") || !std::io::stdout().is_terminal()
        {
            return;
        }
        let id = format!("tjikup-{}", std::process::id());
        let payload = format!(
            "tjikup-status-v1:{}",
            json!({"data":card.data,"body":card.body})
        );
        let title = format!(
            "{} · {}",
            card.title,
            std::env::var("HOSTNAME").unwrap_or_else(|_| "remote".into())
        );
        let message = format!("\x1b]99;i={id}:e=1:a=-report:f={}:n={}:t={}:d=0:p=title;{}\x1b\\\x1b]99;i={id}:e=1:d=0:p=body;{}\x1b\\\x1b]99;i={id}:e=1:d=1:p=buttons;{}\x1b\\", STANDARD.encode("Tjikup"), STANDARD.encode("tjikup-notification"), STANDARD.encode(payload), STANDARD.encode(title), STANDARD.encode(&card.data.state), STANDARD.encode("Focus"));
        let _ = std::io::stdout().write_all(message.as_bytes());
        let _ = std::io::stdout().flush();
    } else if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
    {
        if native(&card, focus).unwrap_or(false) {
            return;
        }
        send(&[
            "notify-send".into(),
            "--app-name=Tjikup".into(),
            "--icon=tjikup-notification".into(),
            card.title,
            card.data.state,
        ]);
    }
}
