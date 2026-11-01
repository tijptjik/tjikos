pub mod client;
pub mod store;
use anyhow::{ensure, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn hash(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
pub fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
pub fn validate_snapraid_review(v: &Value) -> Result<()> {
    ensure!(
        v.as_object().is_some_and(|m| m.len() == 3)
            && text(v, "kind") == "snapraid-review"
            && text(v, "host") == "si",
        "Invalid review action"
    );
    let id = text(v, "review");
    ensure!(
        id.len() == 64
            && id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "Invalid review identifier"
    );
    Ok(())
}
pub fn validate(v: &Value) -> Result<()> {
    ensure!(
        v.is_object() && v.to_string().len() <= 65536,
        "Notification exceeds 64 KiB"
    );
    for (key, max) in [
        ("id", 128),
        ("app", 128),
        ("run", 256),
        ("title", 1024),
        ("body", 16384),
    ] {
        ensure!(
            v[key].as_str().is_some_and(|s| s.len() <= max),
            "Invalid {key}"
        );
    }
    ensure!(
        !text(v, "id").is_empty() && !text(v, "app").is_empty(),
        "Missing notification identity"
    );
    ensure!(
        ["local", "active"].contains(&text(v, "delivery")),
        "Invalid delivery"
    );
    ensure!(v["history"].is_boolean(), "Invalid history option");
    let actions = v["actions"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Invalid actions"))?;
    ensure!(actions.len() <= 16, "Too many actions");
    for action in actions {
        ensure!(
            action.as_object().is_some_and(|m| m
                .keys()
                .all(|k| ["id", "label", "url", "open"].contains(&k.as_str()))),
            "Remote command arguments are forbidden"
        );
        ensure!(text(action, "label").len() <= 160, "Invalid action label");
        if let Some(open) = action.get("open") {
            ensure!(
                action.get("id").is_none() && action.get("url").is_none(),
                "Ambiguous action"
            );
            validate_snapraid_review(open)?;
        } else if let Some(link) = action["url"].as_str() {
            let u = url::Url::parse(link)?;
            ensure!(
                u.scheme() == "https" && u.username().is_empty() && u.password().is_none(),
                "Actions require a safe HTTPS link"
            );
        } else {
            ensure!(
                uuid::Uuid::parse_str(text(action, "id")).is_ok(),
                "Invalid action identifier"
            );
        }
    }
    Ok(())
}
