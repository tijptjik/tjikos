use super::{process::*, protection::*};
use crate::util::{atomic_json, now, private_dir, read_json};
use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Datelike, Duration, FixedOffset, Timelike, Utc};
use clap::Parser;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::PathBuf, process::Command};
#[derive(Parser)]
#[command(
    name = "synctjik app-pull",
    about = "Copy retained application snapshots into an independent verified repository"
)]
struct Cli {
    #[arg(default_value="copy", value_parser=["copy", "health", "check"])]
    action: String,
    #[arg(long)]
    policy: Option<PathBuf>,
    #[arg(long)]
    health_check: bool,
}
#[derive(Deserialize)]
struct Policy {
    state: PathBuf,
    restic: Restic,
    source_repository: String,
    source_password: PathBuf,
    ssh: Vec<String>,
    endpoints: Vec<String>,
    remote_host: String,
    sftp_server: Vec<String>,
    limit_download: u32,
    copy_attempts: u32,
    retry_seconds: u64,
    copy_max_hours: i64,
    health_max_hours: i64,
    weekly_offset_seconds: i32,
    weekly_weekday: u32,
    weekly_hour: u32,
}
fn stamp(v: &Value) -> Result<DateTime<FixedOffset>> {
    Ok(DateTime::parse_from_rfc3339(
        v.as_str().context("Invalid snapshot time")?,
    )?)
}
fn weekly_current(p: &Policy, receipt: &Value, now: DateTime<Utc>) -> bool {
    let Some(offset) = FixedOffset::east_opt(p.weekly_offset_seconds) else {
        return false;
    };
    let local = now.with_timezone(&offset);
    let days = (local.weekday().num_days_from_sunday() + 7 - p.weekly_weekday) % 7;
    let Some(mut due) = (local - Duration::days(days as i64))
        .with_hour(p.weekly_hour)
        .and_then(|t| t.with_minute(0))
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
    else {
        return false;
    };
    if local < due {
        due -= Duration::days(7);
    }
    receipt["dataCheck"] == "passed"
        && receipt["sourceTime"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
        && stamp(&receipt["copiedAt"]).is_ok_and(|copied| copied >= due && copied <= now)
}
impl Policy {
    fn validate(&self) -> Result<()> {
        paths(&[&self.state, &self.source_password])?;
        self.restic.validate()?;
        ensure!(
            !self.ssh.is_empty()
                && !self.endpoints.is_empty()
                && !self.sftp_server.is_empty()
                && self.copy_attempts > 0
                && self.copy_attempts <= 10
                && self.weekly_weekday < 7
                && self.weekly_hour < 24
                && FixedOffset::east_opt(self.weekly_offset_seconds).is_some(),
            "Invalid independent-copy policy"
        );
        ensure!(
            self.copy_max_hours > 0 && self.health_max_hours > 0 && self.limit_download > 0,
            "Invalid copy limits"
        );
        Ok(())
    }
    fn copy(&self, r: &Runner, health: bool) -> Result<()> {
        secret(&self.source_password)?;
        secret(&self.restic.password)?;
        let endpoints = std::env::var("SI_BACKUP_SSH_HOST")
            .map(|e| vec![e])
            .unwrap_or_else(|_| self.endpoints.clone());
        let mut ssh = None;
        for endpoint in endpoints {
            ensure!(!endpoint.starts_with('-'), "Invalid SSH endpoint");
            let mut probe = Command::new("timeout");
            probe
                .args(["40"])
                .args(&self.ssh)
                .args([&endpoint, "hostname"]);
            if let Ok((0, hostname)) = status(r, &mut probe) {
                if hostname == self.remote_host {
                    let mut selected = self.ssh.clone();
                    selected.push(endpoint);
                    ssh = Some(selected);
                    break;
                }
            }
            r.check()?;
        }
        let mut ssh = ssh.context("Source host is unreachable using verified SSH endpoints")?;
        ssh.extend(self.sftp_server.clone());
        let options = vec![
            "--limit-download".into(),
            self.limit_download.to_string(),
            "-o".into(),
            format!("sftp.command={}", shell_join(&ssh)),
        ];
        let remote = vec![
            "--from-repo".into(),
            self.source_repository.clone(),
            "--from-password-file".into(),
            path(&self.source_password),
        ];
        let snapshots: Vec<Value> =
            serde_json::from_slice(&r.run(self.restic.command().args(&options).args([
                "-r",
                &self.source_repository,
                "-p",
                &path(&self.source_password),
                "snapshots",
                "--host",
                &self.restic.host,
                "--tag",
                &self.restic.tag,
                "--json",
            ]))?)?;
        let mut ordered = Vec::new();
        for snapshot in snapshots {
            let time = stamp(&snapshot["time"])?;
            exact_snapshot(snapshot["id"].as_str().context("Invalid source snapshot")?)?;
            ordered.push((time, snapshot));
        }
        ordered.sort_by_key(|(time, _)| *time);
        let (time, snapshot) = ordered.last().context("No source backup found")?;
        let age = Utc::now().signed_duration_since(*time);
        ensure!(
            age >= Duration::zero()
                && age
                    <= Duration::hours(if health {
                        self.health_max_hours
                    } else {
                        self.copy_max_hours
                    }),
            "Server backup is stale or future-dated"
        );
        if health && self.restic.repository.join("config").is_file() {
            let receipt = read_json(&self.state.join("last-success.json")).unwrap_or(Value::Null);
            if weekly_current(self, &receipt, Utc::now())
                && self.restic.snapshots(r)?.iter().any(|s| {
                    s["time"] == receipt["sourceTime"]
                        && (s["original"] == receipt["sourceSnapshot"]
                            || s["id"] == receipt["sourceSnapshot"])
                })
            {
                println!("Server backup is fresh; verified weekly copy is present");
                return Ok(());
            }
        }
        let mut init = options.clone();
        init.push("--copy-chunker-params".into());
        init.extend(remote.clone());
        self.restic.init(r, &init)?;
        for attempt in 1..=self.copy_attempts {
            let result = r.run(
                self.restic
                    .command()
                    .args(&options)
                    .arg("copy")
                    .args(&remote)
                    .args(["--host", &self.restic.host, "--tag", &self.restic.tag]),
            );
            if result.is_ok() {
                break;
            }
            if attempt == self.copy_attempts {
                result?;
            }
            delay(r, self.retry_seconds * attempt as u64)?;
        }
        let local = self.restic.snapshots(r)?;
        for (_, source) in &ordered {
            ensure!(
                local
                    .iter()
                    .any(|s| s["id"] == source["id"] || s["original"] == source["id"]),
                "Copy is incomplete; source snapshot missing locally"
            );
        }
        self.restic.retain(r, &self.restic.tag)?;
        r.run(self.restic.command().args(["check", "--read-data"]))?;
        ensure!(
            self.restic
                .snapshots(r)?
                .iter()
                .any(|s| s["id"] == snapshot["id"] || s["original"] == snapshot["id"]),
            "Latest source snapshot missing after retention"
        );
        atomic_json(
            &self.state.join("last-success.json"),
            &json!({"copiedAt":now(), "sourceSnapshot":snapshot["id"], "sourceTime":snapshot["time"], "dataCheck":"passed"}),
        )?;
        println!("Independent copy retained and fully verified");
        Ok(())
    }
}
pub fn main(args: &[String], r: &Runner) -> Result<()> {
    let cli = Cli::parse_from(
        std::iter::once("synctjik app-pull".to_string()).chain(args.iter().cloned()),
    );
    let p: Policy = policy(
        &cli.policy
            .unwrap_or(home()?.join(".config/synctjik/app-pull.json")),
    )?;
    p.validate()?;
    if cli.action == "check" {
        println!("Independent-copy policy valid");
        return Ok(());
    }
    private_dir(&p.state)?;
    let _lock = Lock::take(&p.state.join("copy.lock"), true, r)?;
    let result = p.copy(r, cli.action == "health" || cli.health_check);
    record_error(&p.state, &result)?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weekly_receipt_boundary_matches_frozen_sunday_hkt_contract() {
        let p: Policy = serde_json::from_value(json!({
            "state":"/fixture", "restic":{"repository":"/fixture/repo","password":"/fixture/password","cache":"/fixture/cache","host":"fixture","tag":"app-state","keep_daily":7,"keep_weekly":4},
            "source_repository":"sftp:fixture:/repo","source_password":"/fixture/password","ssh":["ssh"],"endpoints":["fixture"],"remote_host":"fixture","sftp_server":["sftp-server"],"limit_download":2048,"copy_attempts":3,"retry_seconds":10,"copy_max_hours":36,"health_max_hours":24,"weekly_offset_seconds":28800,"weekly_weekday":0,"weekly_hour":6
        })).unwrap();
        let current = DateTime::parse_from_rfc3339("2026-10-25T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        for (copied, expected) in [
            ("2026-10-24T21:59:59Z", false),
            ("2026-10-24T22:00:00Z", true),
            ("2026-10-25T00:00:01Z", false),
        ] {
            assert_eq!(
                weekly_current(
                    &p,
                    &json!({"copiedAt":copied,"sourceTime":"fixture","dataCheck":"passed"}),
                    current
                ),
                expected
            );
        }
        // Before this Sunday's scheduled time, last week's verified copy is current.
        let before = DateTime::parse_from_rfc3339("2026-10-24T21:59:59Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(weekly_current(
            &p,
            &json!({"copiedAt":"2026-10-17T22:00:00Z","sourceTime":"fixture","dataCheck":"passed"}),
            before
        ));
        assert!(!weekly_current(
            &p,
            &json!({"copiedAt":"2026-10-24T22:00:00Z","sourceTime":"fixture","dataCheck":"failed"}),
            current
        ));
    }
}
