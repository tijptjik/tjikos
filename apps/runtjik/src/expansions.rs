use crate::{discovery::Entry, util::*};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    io::Write,
    process::{Command, Stdio},
};

pub fn request(value: Value) -> Result<Value> {
    let mut child = Command::new("python3")
        .arg(crate::paths::root().join("bin/runtjik-expansions"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .context("Expansion input unavailable")?
        .write_all(serde_json::to_string(&value)?.as_bytes())?;
    let output = child.wait_with_output()?;
    anyhow::ensure!(
        output.status.success(),
        "Expansion editor requires Python PyYAML: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout)?;
    anyhow::ensure!(result["error"].is_null(), "{}", s(&result["error"]));
    Ok(result["ok"].clone())
}

pub fn query(query: &str, demo: bool) -> Result<Vec<Entry>> {
    let entries = if demo {
        json!([{"trigger":";hello","expansion":"Hello, world!"},{"trigger":";thanks","expansion":"Thank you for your help."}])
    } else {
        request(json!({"action":"list"}))?
    };
    let filter = query
        .strip_prefix("/expand")
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let mut rows = vec![Entry {
        id: "Expand:new".into(),
        name: "New".into(),
        subtitle: "Create an expansion".into(),
        icon: "list-add".into(),
        provider: "Expand".into(),
        keywords: String::new(),
        data: json!({"keep_open":true}),
    }];
    for (index, entry) in entries
        .as_array()
        .context("Invalid expansion dictionary")?
        .iter()
        .enumerate()
    {
        if !format!("{} {}", s(&entry["trigger"]), s(&entry["expansion"]))
            .to_lowercase()
            .contains(&filter)
        {
            continue;
        }
        rows.push(Entry {
            id: format!(
                "Expand:{}:{}:{}",
                entry["file"].as_str().unwrap_or("demo"),
                entry["index"].as_u64().unwrap_or(index as u64),
                entry["hash"].as_str().unwrap_or("demo")
            ),
            name: s(&entry["trigger"]).into(),
            subtitle: s(&entry["expansion"]).into(),
            icon: "insert-text".into(),
            provider: "Expand".into(),
            keywords: String::new(),
            data: json!({"keep_open":true,"original":entry}),
        });
    }
    Ok(rows)
}
