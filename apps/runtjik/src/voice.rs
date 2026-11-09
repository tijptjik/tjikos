use crate::{discovery::Entry, util::*};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

fn directory() -> PathBuf {
    xdg("state").parent().unwrap().join("hyprvoice/history")
}
fn row(id: &str, name: &str, subtitle: &str, icon: &str, data: Value) -> Entry {
    Entry {
        id: format!("Voice:{id}"),
        name: name.into(),
        subtitle: subtitle.into(),
        provider: "Voice".into(),
        icon: if icon.starts_with("voice-") {
            crate::paths::root()
                .join("qml/icons")
                .join(format!("{icon}.svg"))
                .to_string_lossy()
                .into_owned()
        } else {
            icon.into()
        },
        keywords: String::new(),
        data,
    }
}
pub fn query(query: &str, demo: bool) -> Result<Vec<Entry>> {
    let tail = query.strip_prefix("/voice").unwrap_or("").trim();
    if tail.is_empty() {
        return Ok(vec![
            row(
                "record",
                "Voice Record",
                "Start recording",
                "voice-record",
                json!({"hide_launcher":true}),
            ),
            row(
                "history",
                "Voice History",
                "Browse and copy previous text",
                "voice-history",
                json!({"keep_open":true}),
            ),
        ]);
    }
    if tail == "settings" {
        return Ok(vec![row(
            "settings",
            "Voice Settings",
            "Open Hyprvoice settings",
            "preferences-system",
            json!({}),
        )]);
    }
    let filter = tail
        .strip_prefix("history")
        .filter(|_| tail == "history" || tail.starts_with("history "))
        .unwrap_or(tail)
        .trim()
        .to_lowercase();
    let mut entries = Vec::new();
    if demo {
        entries.push(("demo-1.json".into(), json!({"created":"2026-11-09T10:00:00Z","text":"Remember to bring the notebook to tomorrow’s meeting."})));
        entries.push(("demo-2.json".into(), json!({"created":"2026-11-09T09:00:00Z","text":"Hello 香港. This is a synthetic voice history example."})));
    } else {
        match fs::read_dir(directory()) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
            Ok(files) => {
                for file in files {
                    let file = file?;
                    if !file.file_type()?.is_file()
                        || file.path().extension().is_none_or(|e| e != "json")
                    {
                        continue;
                    }
                    let Ok(bytes) = fs::read(file.path()) else {
                        continue;
                    };
                    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                        continue;
                    };
                    if s(&value["text"]).trim().is_empty() {
                        continue;
                    }
                    entries.push((file.file_name().to_string_lossy().into_owned(), value));
                }
            }
        }
    }
    entries.sort_by(|a, b| {
        chrono::DateTime::parse_from_rfc3339(s(&b.1["created"]))
            .ok()
            .cmp(&chrono::DateTime::parse_from_rfc3339(s(&a.1["created"])).ok())
            .then_with(|| b.0.cmp(&a.0))
    });
    Ok(entries
        .into_iter()
        .filter(|(_, v)| s(&v["text"]).to_lowercase().contains(&filter))
        .map(|(id, v)| {
            row(
                &id,
                s(&v["text"]),
                &chrono::DateTime::parse_from_rfc3339(s(&v["created"]))
                    .map(|date| {
                        date.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    })
                    .unwrap_or_else(|_| s(&v["created"]).to_owned()),
                "text-x-generic",
                json!({"text":v["text"],"file":id,"keep_open":true}),
            )
        })
        .collect())
}
pub fn activate(entry: &Entry, mode: &str, settings: &Value, demo: bool) -> Result<Value> {
    if entry.id == "Voice:history" {
        return Ok(json!({"open_query":"/voice history"}));
    }
    if let Some(file) = entry.data["file"].as_str() {
        ensure!(
            !file.contains('/') && !file.contains('\\') && file.ends_with(".json"),
            "Invalid history entry"
        );
        if mode == "close" {
            if !demo {
                fs::remove_file(directory().join(file))
                    .context("Could not remove voice history entry")?;
            }
            return Ok(
                json!({"kind":"message","message":"Removed from voice history","removed_entry":entry.id}),
            );
        }
        if !demo {
            binary(
                &[
                    "wl-copy".into(),
                    "--type".into(),
                    "text/plain;charset=utf-8".into(),
                ],
                s(&entry.data["text"]).as_bytes().to_vec(),
                5,
                true,
            )?;
        }
        return Ok(json!({"kind":"activated","message":"Copied voice text","close_launcher":true}));
    }
    ensure!(mode != "close", "Select a history entry to remove");
    if !demo {
        if entry.id == "Voice:settings" || mode == "switch" {
            let mut args = strings(&settings["launch"]["terminal"]);
            ensure!(
                !args.is_empty(),
                "Configure a terminal to open voice settings"
            );
            args.extend(["hyprvoice".into(), "configure".into()]);
            dispatch(args, settings)?;
        } else {
            let status = binary(&["hyprvoice".into(), "status".into()], vec![], 5, false)?;
            ensure!(
                String::from_utf8_lossy(&status).trim() == "STATUS status=idle",
                "Hyprvoice is already recording or processing"
            );
            let response = binary(&["hyprvoice".into(), "toggle".into()], vec![], 5, false)?;
            ensure!(
                !String::from_utf8_lossy(&response).starts_with("ERR"),
                "{}",
                String::from_utf8_lossy(&response)
            );
        }
    }
    Ok(json!({"kind":"activated","demo":demo}))
}
