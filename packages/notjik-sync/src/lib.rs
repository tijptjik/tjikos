pub mod client;
pub mod native;
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
pub fn validate_service_logs(v: &Value) -> Result<()> {
    ensure!(
        v.as_object().is_some_and(|m| m.len() == 3) && text(v, "kind") == "service-logs",
        "Unknown desktop action"
    );
    for key in ["host", "unit"] {
        let s = text(v, key);
        ensure!(
            !s.is_empty()
                && s.len() <= 255
                && s.as_bytes()[0].is_ascii_alphanumeric()
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)),
            "Invalid service log {key}"
        );
    }
    ensure!(
        text(v, "unit").ends_with(".service"),
        "Expected a service unit"
    );
    Ok(())
}
pub fn validate_desktop_action(v: &Value) -> Result<()> {
    match text(v, "kind") {
        "snapraid-review" => validate_snapraid_review(v),
        "service-logs" => validate_service_logs(v),
        _ => anyhow::bail!("Unknown desktop action"),
    }
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
                .all(|k| ["id", "label", "url", "open", "copy"].contains(&k.as_str()))),
            "Remote command arguments are forbidden"
        );
        ensure!(text(action, "label").len() <= 160, "Invalid action label");
        if let Some(copy) = action.get("copy") {
            let value = copy.as_str().unwrap_or("");
            ensure!(
                (4..=32).contains(&value.len())
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b" -".contains(&b)),
                "Invalid copy code"
            );
            ensure!(
                ["id", "url", "open"]
                    .iter()
                    .all(|k| action.get(k).is_none()),
                "Ambiguous copy action"
            );
        } else if let Some(open) = action.get("open") {
            ensure!(
                action.get("id").is_none() && action.get("url").is_none(),
                "Ambiguous desktop action"
            );
            validate_desktop_action(open)?;
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

#[cfg(test)]
mod copy_tests {
    use super::*;
    #[test]
    fn copy_is_local_data_not_a_remote_job() {
        let mut n = serde_json::json!({"id":"sample","app":"gmailtjik","run":"sample","title":"Code","body":"Synthetic","delivery":"active","history":false,"actions":[{"label":"Copy code","copy":"004281"}]});
        assert!(validate(&n).is_ok());
        n["actions"][0]["copy"] = serde_json::json!("$(bad)");
        assert!(validate(&n).is_err());
        n["actions"][0]["copy"] = serde_json::json!("004281");
        n["actions"][0]["id"] = serde_json::json!(id());
        assert!(validate(&n).is_err());
    }
}
