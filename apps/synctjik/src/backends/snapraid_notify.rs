use super::{process::*, protection::*};
use crate::util::{atomic_json, now, private_dir, read_json};
use anyhow::{bail, ensure, Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
#[derive(Parser)]
#[command(
    name = "synctjik snapraid-notify",
    about = "Maintenance incident, recovery and missed-run notifications"
)]
struct Cli {
    #[arg(value_parser=["check", "failure", "recovery", "health"])]
    action: String,
    unit: Option<String>,
    #[arg(
        long,
        default_value = "/etc/si-snapraid-maintenance/notifications.json"
    )]
    policy: PathBuf,
}
#[derive(Deserialize)]
struct Job {
    task: String,
    mode: Option<String>,
    max_age: Option<i64>,
    #[serde(default)]
    test: bool,
    #[serde(default)]
    remain_after_exit: bool,
}
#[derive(Deserialize)]
struct Policy {
    host: String,
    state: PathBuf,
    receipts: PathBuf,
    credentials: PathBuf,
    jobs: BTreeMap<String, Job>,
    reminder_seconds: i64,
    telegram_base_url: String,
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}
fn optional(p: &Path) -> Result<Value> {
    if p.exists() {
        read_json(p)
    } else {
        Ok(json!({}))
    }
}
fn age(s: &Value) -> Option<i64> {
    DateTime::parse_from_rfc3339(s.as_str()?)
        .ok()
        .map(|s| Utc::now().signed_duration_since(s).num_seconds())
}
fn unit_status(r: &Runner, unit: &str) -> Result<BTreeMap<String, String>> {
    Ok(text(
        r,
        Command::new("timeout").args([
            "10",
            "systemctl",
            "show",
            unit,
            "-p",
            "ActiveState",
            "-p",
            "Result",
            "-p",
            "ExecMainStatus",
            "-p",
            "ExecMainStartTimestampMonotonic",
        ]),
    )?
    .lines()
    .filter_map(|l| l.split_once('=').map(|(k, v)| (k.into(), v.into())))
    .collect())
}
impl Policy {
    fn validate(&self) -> Result<()> {
        paths(&[&self.state, &self.receipts, &self.credentials])?;
        ensure!(
            !self.jobs.is_empty() && self.reminder_seconds > 0,
            "Invalid notification policy"
        );
        for (unit, job) in &self.jobs {
            crate::util::host(unit)?;
            ensure!(unit.ends_with(".service"), "Invalid maintenance unit");
            if let Some(mode) = &job.mode {
                crate::util::host(mode)?;
                ensure!(
                    job.max_age.is_some_and(|n| n > 0),
                    "Missing job freshness limit"
                );
            }
        }
        let url = reqwest::Url::parse(&self.telegram_base_url)?;
        ensure!(
            url.scheme() == "https"
                || (url.scheme() == "http"
                    && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))),
            "Notification endpoint requires HTTPS"
        );
        Ok(())
    }
    fn format(&self, unit: &str, problem: Option<&str>) -> String {
        let job = &self.jobs[unit];
        if job.test {
            return format!(
                "🧪 <b>SnapRAID · {}</b>\n<code>{}</code> · Notification check\n\n{}",
                if problem.is_some() {
                    "Test alert"
                } else {
                    "Test passed"
                },
                escape(&self.host),
                if problem.is_some() {
                    "This is a notification test. No storage job failed."
                } else {
                    "Failure and recovery notifications are working."
                }
            );
        }
        let heading = format!(
            "<code>{}</code> · {}",
            escape(&self.host),
            escape(&job.task)
        );
        match problem {
            None => format!("✅ <b>SnapRAID · Recovered</b>\n{heading}\n\nMaintenance completed successfully."),
            Some(problem) => format!("⚠️ <b>SnapRAID · Action needed</b>\n{heading}\n\n{}\n\n<b>Review logs</b>\n<pre>journalctl -u {} -n 50</pre>", escape(problem), escape(unit.trim_end_matches(".service"))),
        }
    }
    fn send(&self, message: &str) -> Result<Value> {
        let credentials: Value = serde_json::from_str(&secret(&self.credentials)?)?;
        let token = credentials["bot_token"]
            .as_str()
            .context("Missing notification token")?;
        ensure!(
            credentials.get("chat_id").is_some(),
            "Missing notification destination"
        );
        let mut payload = json!({"chat_id":credentials["chat_id"], "text":message, "disable_notification":false, "parse_mode":"HTML"});
        if let Some(topic) = credentials
            .get("topic_id")
            .filter(|v| !v.is_null() && **v != 0)
        {
            payload["message_thread_id"] = topic.clone();
        }
        // Never propagate reqwest errors: their URLs contain the bot token.
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| anyhow::anyhow!("Notification transport unavailable"))?;
        let response = client
            .post(format!(
                "{}/bot{token}/sendMessage",
                self.telegram_base_url.trim_end_matches('/')
            ))
            .json(&payload)
            .send()
            .map_err(|_| anyhow::anyhow!("Telegram delivery failed"))?;
        ensure!(
            response.status().is_success(),
            "Telegram delivery failed: HTTP {}",
            response.status().as_u16()
        );
        let body: Value = response
            .json()
            .map_err(|_| anyhow::anyhow!("Invalid Telegram response"))?;
        ensure!(
            body["ok"] == true && body["result"]["message_id"].is_number(),
            "Telegram rejected notification"
        );
        Ok(body["result"]["message_id"].clone())
    }
    fn incident(&self, unit: &str, problem: &str) -> Result<()> {
        let path = self.state.join(format!("{unit}.json"));
        let mut state = optional(&path)?;
        if state["active"] == true
            && state["problem"] == problem
            && age(&state["last_sent_at"]).is_some_and(|n| n >= 0 && n < self.reminder_seconds)
        {
            return Ok(());
        }
        state["active"] = json!(true);
        state["problem"] = json!(problem);
        state["last_sent_at"] = Value::Null;
        atomic_json(&path, &state)?;
        let id = self.send(&self.format(unit, Some(problem)))?;
        state["last_sent_at"] = json!(now());
        state["message_id"] = id;
        atomic_json(&path, &state)
    }
    fn recover(&self, unit: &str) -> Result<()> {
        let path = self.state.join(format!("{unit}.json"));
        if optional(&path)?["active"] != true {
            return Ok(());
        }
        let id = self.send(&self.format(unit, None))?;
        atomic_json(
            &path,
            &json!({"active":false, "recovered_at":now(), "message_id":id}),
        )
    }
    fn reason(&self, unit: &str, status: &BTreeMap<String, String>) -> String {
        if let Some(mode) = &self.jobs[unit].mode {
            if let Ok(record) = read_json(&self.receipts.join(format!("{mode}-failure.json"))) {
                if age(&record["failed_at"]).is_some_and(|a| (0..=600).contains(&a)) {
                    if let Some(error) = record["error"].as_str() {
                        return error.chars().take(450).collect();
                    }
                }
            }
        }
        format!(
            "Job failed or a safety check blocked it. Result={}; exit={}.",
            status
                .get("Result")
                .map(String::as_str)
                .unwrap_or("unknown"),
            status
                .get("ExecMainStatus")
                .map(String::as_str)
                .unwrap_or("unknown")
        )
    }
    fn health(&self, r: &Runner) -> Result<()> {
        for (unit, job) in &self.jobs {
            let Some(mode) = &job.mode else {
                if job.remain_after_exit
                    && optional(&self.state.join(format!("{unit}.json")))?["active"] == true
                {
                    let status = unit_status(r, unit)?;
                    if status.get("Result").map(String::as_str) == Some("success")
                        && status.get("ActiveState").map(String::as_str) == Some("active")
                    {
                        self.recover(unit)?;
                    }
                }
                continue;
            };
            let limit = job.max_age.context("Missing freshness limit")?;
            let status = unit_status(r, unit)?;
            let state = status.get("ActiveState").map(String::as_str).unwrap_or("");
            if state == "failed" {
                self.incident(unit, &self.reason(unit, &status))?;
                continue;
            }
            if state == "active" || state == "activating" {
                let start = status
                    .get("ExecMainStartTimestampMonotonic")
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0) as f64
                    / 1e6;
                let mut ts = libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                };
                ensure!(
                    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) } == 0,
                    "Cannot read monotonic clock"
                );
                if start > 0. && ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9 - start > limit as f64 {
                    self.incident(
                        unit,
                        &format!(
                            "Job has been running longer than {} hours; inspect it.",
                            limit / 3600
                        ),
                    )?;
                }
                continue;
            }
            let receipt = read_json(&self.receipts.join(format!("last-{mode}-success.json")))
                .unwrap_or(Value::Null);
            if age(&receipt["completed_at"]).is_some_and(|n| n >= 0 && n <= limit) {
                self.recover(unit)?;
            } else {
                self.incident(unit, &format!("No verified successful {mode} within {} hours. Check its timer, dependencies and logs.", limit / 3600))?;
            }
        }
        Ok(())
    }
}
pub fn main(args: &[String], r: &Runner) -> Result<()> {
    let cli = Cli::parse_from(
        std::iter::once("synctjik snapraid-notify".to_string()).chain(args.iter().cloned()),
    );
    let p: Policy = policy(&cli.policy)?;
    p.validate()?;
    if cli.action == "check" {
        println!("Maintenance notification policy valid");
        return Ok(());
    }
    private_dir(&p.state)?;
    let _lock = Lock::take(&p.state.join("notify.lock"), true, r)?;
    if cli.action == "health" {
        return p.health(r);
    }
    let unit = cli.unit.context("Supply a maintenance unit")?;
    ensure!(p.jobs.contains_key(&unit), "Unknown maintenance unit");
    let status = unit_status(r, &unit)?;
    let recovered = status.get("Result").map(String::as_str) == Some("success")
        && status.get("ActiveState").map(String::as_str) != Some("failed");
    match cli.action.as_str() {
        "failure" if !recovered => p.incident(&unit, &p.reason(&unit, &status)),
        "failure" | "recovery" if recovered => p.recover(&unit),
        "recovery" => Ok(()),
        _ => bail!("Unknown notification action"),
    }
}
