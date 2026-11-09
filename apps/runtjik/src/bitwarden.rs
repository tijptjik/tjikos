use crate::{discovery::Entry, util::*};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::process::Command;

fn open_item(id: &str) -> Result<()> {
    let gio = which("gio").context("gio is required to open Bitwarden items")?;
    let deep_link = format!("bitwarden://vault/items/{id}");
    if Command::new(&gio)
        .args(["open", &deep_link])
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return Ok(());
    }

    let base = std::env::var("RUNTJIK_BITWARDEN_VAULT_URL")
        .unwrap_or_else(|_| "https://vault.bitwarden.com".into());
    let mut url = url::Url::parse(&base).context("Invalid Bitwarden vault URL")?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https"),
        "Bitwarden vault URL must use HTTP or HTTPS"
    );
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("itemId", id)
        .finish();
    url.set_fragment(Some(&format!("/vault?{query}")));
    let output = Command::new(gio)
        .args(["open", url.as_str()])
        .output()
        .context("Could not open the Bitwarden web vault")?;
    anyhow::ensure!(
        output.status.success(),
        "Could not open the Bitwarden item in the web vault"
    );
    Ok(())
}

fn row(id: &str, name: &str, subtitle: &str, data: Value) -> Entry {
    Entry {
        id: format!("Bitwarden:{id}"),
        name: name.into(),
        subtitle: subtitle.into(),
        icon: crate::paths::command_icon("bw"),
        provider: "Bitwarden".into(),
        keywords: String::new(),
        data,
    }
}

fn bw(args: &[&str]) -> Result<Vec<u8>> {
    let executable = which("bw").context("Install the Bitwarden CLI to use /bw")?;
    let output = Command::new(executable)
        .args(args)
        .output()
        .context("Could not start Bitwarden CLI")?;
    anyhow::ensure!(
        output.status.success(),
        "Bitwarden CLI is locked or the requested action was denied"
    );
    Ok(output.stdout)
}

pub fn query(query: &str, demo: bool) -> Result<Vec<Entry>> {
    let search = query.trim().strip_prefix("/bw").unwrap_or("").trim();
    if demo {
        return Ok(if search.is_empty() {
            vec![row(
                "hint",
                "Search the vault",
                "Synthetic demo data only",
                json!({"disabled":true}),
            )]
        } else {
            vec![row(
                "demo",
                "Demo login",
                "example.invalid · demo-user",
                json!({"username":"demo-user","url":"https://example.invalid","reprompt":false,"demo":true,"keep_open":true}),
            )]
        });
    }
    let status = bw(&["status"])?;
    let status: Value =
        serde_json::from_slice(&status).context("Bitwarden returned invalid status data")?;
    if status["status"] != "unlocked" {
        return Ok(vec![row(
            "locked",
            "Unlock Bitwarden first",
            "Open the vault and unlock it to search credentials",
            json!({"disabled":true}),
        )]);
    }
    if search.is_empty() {
        return Ok(vec![row(
            "hint",
            "Search the vault",
            "Only item names, usernames, and site names appear in results",
            json!({"disabled":true}),
        )]);
    }
    let data = bw(&["list", "items", "--search", search])?;
    let items: Vec<Value> =
        serde_json::from_slice(&data).context("Bitwarden returned invalid item data")?;
    let mut rows = Vec::new();
    for item in items {
        if item["type"].as_i64().unwrap_or(0) != 1 {
            continue;
        }
        let Some(id) = item["id"].as_str() else {
            continue;
        };
        let name = item["name"].as_str().unwrap_or("Login");
        let username = item["login"]["username"].as_str().unwrap_or("");
        let url = item["login"]["uris"][0]["uri"].as_str().unwrap_or("");
        let host = url::Url::parse(url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_default();
        let reprompt = item["reprompt"].as_i64().unwrap_or(0) != 0;
        let has_totp = item["login"]["totp"]
            .as_str()
            .is_some_and(|totp| !totp.is_empty());
        rows.push(row(id, name, &format!("{}{}{}", if host.is_empty() { "" } else { &host }, if !host.is_empty() && !username.is_empty() { " · " } else { "" }, username), json!({"item_id":id,"username":username,"url":url,"reprompt":reprompt,"has_totp":has_totp,"keep_open":true})));
    }
    if rows.is_empty() {
        rows.push(row(
            "none",
            "No matching logins",
            "Search matches item names and the Bitwarden site filter",
            json!({"disabled":true}),
        ));
    }
    Ok(rows)
}

pub fn activate(entry: &Entry, mode: &str, demo: bool) -> Result<Value> {
    if demo {
        return Ok(
            json!({"kind":"message","message":"Demo mode leaves the Bitwarden vault and clipboard unchanged"}),
        );
    }
    let id = s(&entry.data["item_id"]);
    anyhow::ensure!(!id.is_empty(), "Select a Bitwarden login");
    if mode == "close" {
        open_item(id)?;
        return Ok(json!({"kind":"message","message":"Opened the Bitwarden item"}));
    }
    match mode {
        "launch" => {
            anyhow::ensure!(
                entry.data["reprompt"] != true,
                "This item requires a password reprompt in Bitwarden; open the vault to continue"
            );
            let password = bw(&["get", "password", id])?;
            let password = String::from_utf8_lossy(&password).trim_end().to_owned();
            anyhow::ensure!(!password.is_empty(), "This login has no password");
            crate::clipboard::copy_text(&password, true)?;
            Ok(
                json!({"kind":"message","message":"Password copied; clipboard clears after one paste"}),
            )
        }
        "switch" => {
            anyhow::ensure!(
                entry.data["reprompt"] != true,
                "This item requires a password reprompt in Bitwarden; open the vault to continue"
            );
            anyhow::ensure!(
                entry.data["has_totp"] == true,
                "This login has no TOTP code"
            );
            let code = bw(&["get", "totp", id])?;
            let code = String::from_utf8_lossy(&code).trim().to_owned();
            anyhow::ensure!(!code.is_empty(), "Bitwarden did not return a TOTP code");
            crate::clipboard::copy_text(&code, true)?;
            Ok(json!({"kind":"message","message":"TOTP copied; clipboard clears after one paste"}))
        }
        "username" => {
            let username = s(&entry.data["username"]);
            anyhow::ensure!(!username.is_empty(), "This login has no username");
            crate::clipboard::copy_text(username, true)?;
            Ok(json!({"kind":"message","message":"Username copied"}))
        }
        _ => anyhow::bail!("Unknown Bitwarden action"),
    }
}
