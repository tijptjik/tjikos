use super::{process::*, protection::*};
use crate::util::{atomic_json, now, private_dir, read_json};
use anyhow::{ensure, Context, Result};
use clap::Parser;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{fs, path::PathBuf, process::Command};
#[derive(Parser)]
#[command(
    name = "synctjik snapraid",
    about = "Guarded parity maintenance and recovery of owned writer pauses"
)]
struct Cli {
    #[arg(value_parser=["check", "guard", "status", "preflight", "sync", "scrub", "resume"])]
    action: String,
    #[arg(long, default_value = "/etc/si-snapraid-maintenance/config.json")]
    policy: PathBuf,
}
#[derive(Deserialize)]
struct Policy {
    state: PathBuf,
    lock: PathBuf,
    backup_lock: PathBuf,
    log_dir: PathBuf,
    snapraid_config: PathBuf,
    layout: Vec<Vec<String>>,
    writers: Vec<String>,
    max_removed: u64,
    #[serde(default)]
    max_changed: Option<u64>,
    scrub_percent: u32,
    scrub_age_days: u32,
    #[serde(flatten)]
    mounts: Mounts,
}
fn summary(text: &str, label: &str) -> Result<u64> {
    let counts: Vec<_> = text
        .lines()
        .filter_map(|l| {
            let parts: Vec<_> = l.split_whitespace().collect();
            if parts.len() == 2 && parts[1] == label {
                parts[0].parse::<u64>().ok()
            } else {
                None
            }
        })
        .collect();
    ensure!(counts.len() == 1, "Missing or ambiguous diff summary");
    Ok(counts[0])
}
impl Policy {
    fn validate(&self) -> Result<()> {
        paths(&[
            &self.state,
            &self.lock,
            &self.backup_lock,
            &self.log_dir,
            &self.snapraid_config,
        ])?;
        ensure!(
            !self.mounts.mounts.is_empty()
                && !self.layout.is_empty()
                && self.scrub_percent > 0
                && self.scrub_percent <= 100,
            "Incomplete SnapRAID protection policy"
        );
        ensure!(
            self.lock != self.backup_lock,
            "Maintenance and backup locks must differ"
        );
        Ok(())
    }
    fn guard(&self, r: &Runner) -> Result<()> {
        self.mounts.check(r)?;
        let actual: Vec<Vec<String>> = fs::read_to_string(&self.snapraid_config)?
            .lines()
            .map(|l| l.split_whitespace().map(String::from).collect::<Vec<_>>())
            .filter(|w| {
                w.first().is_some_and(|s| {
                    ["data", "1-parity", "2-parity", "content"].contains(&s.as_str())
                })
            })
            .collect();
        ensure!(
            actual == self.layout,
            "SnapRAID layout changed; review maintenance policy"
        );
        for entry in glob::glob("/proc/[0-9]*/comm")? {
            match fs::read_to_string(entry?) {
                Ok(name) => ensure!(
                    name.trim() != "snapraid",
                    "Another SnapRAID process is running"
                ),
                Err(e)
                    if [std::io::ErrorKind::NotFound].contains(&e.kind())
                        || e.raw_os_error() == Some(libc::ESRCH) =>
                {
                    ()
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    fn resume(&self, r: &Runner) -> Result<()> {
        let paused = self.state.join("paused.json");
        if !paused.exists() {
            return Ok(());
        }
        let ids: Vec<String> = serde_json::from_value(read_json(&paused)?)?;
        let mut failures = Vec::new();
        for id in &ids {
            ensure!(
                !id.starts_with('-') && !id.is_empty(),
                "Invalid pause ownership record"
            );
            match status(
                r,
                Command::new("docker").args(["inspect", "--format", "{{.State.Paused}}", id]),
            ) {
                Ok((0, state)) if state == "false" => (),
                Ok((0, state)) if state == "true" => {
                    if r.run(Command::new("docker").args(["unpause", id])).is_err() {
                        failures.push(id);
                    }
                }
                _ => failures.push(id),
            }
        }
        ensure!(
            failures.is_empty(),
            "Could not resume all owned writers; pause record retained"
        );
        remove(&paused)
    }
    fn pause(&self, r: &Runner) -> Result<()> {
        let running = text(r, Command::new("docker").args(["ps", "-q"]))?;
        let ids: Vec<_> = running.split_whitespace().collect();
        let containers: Vec<Value> = if ids.is_empty() {
            vec![]
        } else {
            serde_json::from_slice(&r.run(Command::new("docker").arg("inspect").args(ids))?)?
        };
        let mut owned = Vec::new();
        for c in containers {
            let name = c["Name"]
                .as_str()
                .context("Invalid container name")?
                .trim_start_matches('/');
            if self.writers.iter().any(|n| n == name) && c["State"]["Paused"] == false {
                owned.push(
                    c["Id"]
                        .as_str()
                        .context("Missing container identity")?
                        .to_string(),
                );
            }
        }
        atomic_json(&self.state.join("paused.json"), &json!(owned))?;
        for id in owned {
            r.run(Command::new("docker").args(["pause", &id]))?;
        }
        Ok(())
    }
    fn maintain(&self, r: &Runner, action: &str) -> Result<()> {
        self.guard(r)?;
        ensure!(
            action != "sync" || !self.state.join("scrub-failure.json").exists(),
            "Previous scrub failed; investigate and complete a successful scrub before syncing"
        );
        if action == "guard" {
            println!("Disk identities and configuration layout verified");
            return Ok(());
        }
        let _backup = Lock::take(&self.backup_lock, true, r)?;
        self.guard(r)?;
        self.resume(r)?;
        private_dir(&self.log_dir)?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let result = (|| {
            if action == "sync" || action == "preflight" {
                if action == "sync" {
                    self.pause(r)?;
                }
                let log = self.log_dir.join(format!("{stamp}-diff.log"));
                no_symlinks(&log)?;
                let output = tempfile::NamedTempFile::new_in(&self.log_dir)?;
                let (code, diff) = r.logged(
                    Command::new("snapraid")
                        .arg("-c")
                        .arg(&self.snapraid_config)
                        .arg("diff"),
                    output.as_file(),
                )?;
                // Keep the diff private; do not emit protected filenames to the UI.
                output.persist(&log)?;
                let diff = String::from_utf8(diff)?;
                ensure!(code == 0 || code == 2, "SnapRAID diff failed; inspect log");
                let removed = summary(&diff, "removed")?;
                ensure!(
                    removed <= self.max_removed,
                    "{removed} removals exceed limit {}; review diff before any sync",
                    self.max_removed
                );
                if let Some(limit) = self.max_changed {
                    ensure!(
                        summary(&diff, "updated")? <= limit,
                        "Changed files exceed policy limit"
                    );
                }
                if action == "preflight" {
                    println!("Diff gate passed");
                    return Ok(());
                }
                r.run(
                    Command::new("snapraid")
                        .arg("-c")
                        .arg(&self.snapraid_config)
                        .arg("-l")
                        .arg(self.log_dir.join(format!("{stamp}-sync.log")))
                        .arg("sync"),
                )?;
            } else {
                r.run(
                    Command::new("snapraid")
                        .arg("-c")
                        .arg(&self.snapraid_config)
                        .arg("-l")
                        .arg(self.log_dir.join(format!("{stamp}-scrub.log")))
                        .args([
                            "-p",
                            &self.scrub_percent.to_string(),
                            "-o",
                            &self.scrub_age_days.to_string(),
                            "scrub",
                        ]),
                )?;
            }
            Ok(())
        })();
        let cleanup = self.resume(&Runner::new()?);
        let result: Result<()> = result.and(cleanup);
        if let Err(e) = &result {
            atomic_json(
                &self.state.join(format!("{action}-failure.json")),
                &json!({"failed_at":now(), "error":e.to_string()}),
            )?;
        } else if action != "preflight" {
            atomic_json(
                &self.state.join(format!("last-{action}-success.json")),
                &json!({"completed_at":now(), "mode":action}),
            )?;
            remove(&self.state.join(format!("{action}-failure.json")))?;
        }
        result
    }
}
pub fn main(args: &[String], r: &Runner) -> Result<()> {
    let cli = Cli::parse_from(
        std::iter::once("synctjik snapraid".to_string()).chain(args.iter().cloned()),
    );
    let p: Policy = policy(&cli.policy)?;
    p.validate()?;
    if cli.action == "check" {
        println!("SnapRAID protection policy valid");
        return Ok(());
    }
    let _lock = Lock::take(&p.lock, cli.action != "status", r)?;
    if cli.action == "status" {
        p.guard(r)?;
        print!(
            "{}",
            text(
                r,
                Command::new("snapraid")
                    .arg("-c")
                    .arg(&p.snapraid_config)
                    .arg("status")
            )?
        );
        return Ok(());
    }
    private_dir(&p.state)?;
    if cli.action == "resume" {
        return p.resume(r);
    }
    p.maintain(r, &cli.action)
}
