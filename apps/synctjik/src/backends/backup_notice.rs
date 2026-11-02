//! Public backup summaries. Private dependency output stays in receipts/journals.
use super::{process::Runner, protection::text};
use crate::util::{atomic_json, now, read_json};
use anyhow::Result;
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path, process::Command};

fn public_error(error: &str) -> &str {
    if error.contains("Server backup is stale or future-dated") {
        "The server has no backup within the required freshness window. No new verified copy was recorded."
    } else if error.contains("unreachable using verified SSH endpoints") {
        "The server could not be reached through its verified SSH endpoints."
    } else if error.contains("Another backup or sync is running") {
        "Storage maintenance holds the backup lock. This attempt was deferred before copying data."
    } else if error.contains("Copy is incomplete") {
        "The independent copy is incomplete. Its verified-success receipt was not advanced."
    } else {
        "The job did not complete. Inspect its service journal and private error receipt."
    }
}

fn card(
    host: &str,
    unit: &str,
    success: bool,
    error: Option<&str>,
    receipt: &Value,
    status: &BTreeMap<String, String>,
) -> Value {
    let job = match unit {
        "si-app-backup.service" => "Application backup",
        "si-app-backup-pull.service" => "Independent backup copy",
        _ => "Backup health check",
    };
    let body = if success {
        if unit == "si-app-backup.service" {
            "Application state was backed up and the repository structure checked."
        } else if unit == "si-app-backup-pull.service" {
            "Retained server snapshots were copied and all local repository data verified."
        } else {
            "The server backup is fresh and the verified weekly copy is present."
        }
    } else if status.get("Result").is_some_and(|s| s == "timeout") {
        "The job exceeded its time limit. No successful completion was recorded for this attempt."
    } else {
        public_error(error.unwrap_or_default())
    };
    let mut rows = vec![
        json!({"label":"Host","value":host}),
        json!({"label":"Job","value":job}),
    ];
    for (key, label) in [
        ("completedAt", "Last backup"),
        ("copiedAt", "Last verified copy"),
        ("sourceTime", "Copied snapshot"),
    ] {
        if let Some(value) = receipt[key].as_str().filter(|s| !s.is_empty()) {
            if let Ok(time) = chrono::DateTime::parse_from_rfc3339(value) {
                rows.push(json!({"label":label,"value":time.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M %Z").to_string()}));
            }
        }
    }
    if receipt["dataCheck"] == "passed" {
        rows.push(json!({"label":"Last copy verification","value":"All data passed"}));
    }
    if !success {
        let retry = match (
            status.get("Restart").map(String::as_str),
            status.get("RestartUSec"),
        ) {
            (Some("on-failure" | "always"), Some(delay)) => {
                format!("Automatic retry after {delay}")
            }
            (Some("no"), _) => "No automatic retry; next scheduled run or manual start".into(),
            _ => "Retry schedule unavailable; inspect the service".into(),
        };
        rows.push(json!({"label":"Next attempt","value":retry}));
    }
    let actions = if !success
        && (unit == "si-app-backup.service"
            || error.is_some_and(|e| e.contains("Server backup is stale")))
    {
        vec![
            json!({"label":"Investigate server","open":{"kind":"service-logs","host":if unit == "si-app-backup.service" {host} else {"si"},"unit":"si-app-backup.service"}}),
        ]
    } else {
        vec![]
    };
    let deferred =
        !success && error.is_some_and(|e| e.contains("Another backup or sync is running"));
    json!({"title":format!("{job} · {host}"),"body":body,"app":"Synctjik","type":"results-table","run":format!("backup:{host}:{unit}"),
        "data":{"state":if success {"Complete"} else if deferred {"Deferred"} else {"Attention needed"},"tone":if success {"success"} else if deferred {"warning"} else {"critical"},"results":rows},
        "actions":actions,"urgency":if success || deferred {"normal"} else {"critical"}})
}

pub fn publish(
    state: &Path,
    host: &str,
    unit: &str,
    user: bool,
    success: bool,
    r: &Runner,
) -> Result<()> {
    let mut command = Command::new("systemctl");
    if user {
        command.arg("--user");
    }
    command.args([
        "show",
        unit,
        "-p",
        "Result",
        "-p",
        "Restart",
        "-p",
        "RestartUSec",
        "-p",
        "InvocationID",
    ]);
    let mut status: BTreeMap<String, String> = text(r, &mut command)
        .unwrap_or_default()
        .lines()
        .filter_map(|s| s.split_once('=').map(|(k, v)| (k.into(), v.into())))
        .collect();
    if let Ok(result) = std::env::var("SERVICE_RESULT") {
        status.insert("Result".into(), result);
    }
    let receipt = read_json(&state.join("last-success.json")).unwrap_or(Value::Null);
    let error = read_json(&state.join("last-error.json")).unwrap_or(Value::Null);
    // OnFailure may run after a timeout before the worker wrote a receipt.
    // Never attribute an older attempt's error to the current invocation.
    let invocation = std::env::var("MONITOR_INVOCATION_ID")
        .ok()
        .or_else(|| std::env::var("INVOCATION_ID").ok())
        .or_else(|| status.get("InvocationID").cloned());
    let current = invocation
        .as_deref()
        .filter(|s| !s.is_empty())
        .is_some_and(|id| error["invocation"] == id);
    let notice = card(
        host,
        unit,
        success,
        if current {
            error["error"].as_str()
        } else {
            None
        },
        &receipt,
        &status,
    );
    if notjik_sync::client::configured() {
        notjik_sync::client::publish(notice, "active", true)?;
    } else {
        // Preserve a readable diagnostic when a machine is not enrolled.
        eprintln!(
            "{}: {}",
            notice["title"].as_str().unwrap(),
            notice["body"].as_str().unwrap()
        );
    }
    // Copy and health share backup receipts, but their cards recover separately.
    // A successful copy must not erase the health card's pending recovery.
    let marker = state.join(format!("notice-{unit}.json"));
    if success {
        super::protection::remove(&marker)?;
    } else {
        atomic_json(&marker, &json!({"failed_at":now()}))?;
    }
    Ok(())
}

pub fn pending(state: &Path, unit: &str) -> bool {
    state.join(format!("notice-{unit}.json")).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_health_keeps_verified_copy_and_actual_retry_policy() {
        let c = card(
            "fi",
            "si-app-backup-health.service",
            false,
            Some("Server backup is stale or future-dated"),
            &json!({"copiedAt":"2026-11-01T00:00:00Z","dataCheck":"passed"}),
            &BTreeMap::from([("Restart".into(), "no".into())]),
        );
        assert_eq!(c["type"], "results-table");
        assert!(c.to_string().contains("No automatic retry"));
        assert!(!c.to_string().contains("hourly"));
        assert_eq!(c["actions"][0]["open"]["host"], "si");
        assert!(c.to_string().contains("All data passed"));
    }
    #[test]
    fn optional_fields_are_omitted_and_private_errors_are_not_sent() {
        let c = card(
            "si",
            "si-app-backup.service",
            false,
            Some("private-file secret diagnostic"),
            &Value::Null,
            &BTreeMap::from([
                ("Restart".into(), "on-failure".into()),
                ("RestartUSec".into(), "1h".into()),
            ]),
        );
        assert!(!c.to_string().contains("private-file"));
        assert_eq!(c["data"]["results"].as_array().unwrap().len(), 3);
        assert!(c.to_string().contains("Automatic retry after 1h"));
    }
    #[test]
    fn server_success_does_not_claim_full_data_verification() {
        let c = card(
            "si",
            "si-app-backup.service",
            true,
            None,
            &Value::Null,
            &BTreeMap::new(),
        );
        assert!(c["body"].as_str().unwrap().contains("structure checked"));
        assert!(c["actions"].as_array().unwrap().is_empty());
    }
}
