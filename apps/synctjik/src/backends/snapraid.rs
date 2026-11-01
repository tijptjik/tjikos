use super::{process::*, protection::*};
use crate::util::{atomic_json, now, private_dir, read_json};
use anyhow::{ensure, Context, Result};
use clap::Parser;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{fs, path::PathBuf, process::Command};

// Only expose numeric summaries, never protected filenames or raw diagnostics.
fn command_failure(action: &str, code: i32, log: &std::path::Path) -> String {
    let contents = fs::read_to_string(log).unwrap_or_default();
    let count = |key: &str| -> Option<u64> {
        let prefix = format!("summary:{key}:");
        let values: Vec<_> = contents
            .lines()
            .filter_map(|line| line.strip_prefix(&prefix))
            .collect();
        (values.len() == 1)
            .then(|| values[0].parse().ok())
            .flatten()
    };
    let details = match (count("error_soft"), count("error_io"), count("error_data")) {
        (Some(soft), Some(io), Some(data)) => {
            let missing: Vec<_> = contents
                .lines()
                .filter_map(|line| line.strip_prefix("msg:error: Missing file '"))
                .filter_map(|line| line.strip_suffix("'."))
                .collect();
            let missing_details = if soft > 0 && missing.len() as u64 == soft {
                let unique: std::collections::BTreeSet<_> = missing.into_iter().collect();
                format!(" ({} missing files)", unique.len())
            } else {
                String::new()
            };
            format!(": {soft} soft errors{missing_details}, {io} I/O errors, {data} data errors")
        }
        _ => String::new(),
    };
    format!(
        "SnapRAID {action} failed (exit {code}){details}. Log: {}",
        log.display()
    )
}

fn run_maintenance(
    r: &Runner,
    action: &str,
    log: &std::path::Path,
    command: &mut Command,
) -> Result<()> {
    let (code, _) = r.guarded(command, None, true).map_err(|e| {
        anyhow::anyhow!(
            "SnapRAID {action} interrupted or could not run; log: {} ({e})",
            log.display()
        )
    })?;
    ensure!(code == 0, "{}", command_failure(action, code, log));
    Ok(())
}
#[derive(Parser)]
#[command(
    name = "synctjik snapraid",
    about = "Guarded parity maintenance and recovery of owned writer pauses"
)]
struct Cli {
    #[arg(value_parser=["check", "guard", "status", "preflight", "sync", "scrub", "resume", "review", "approve-removals"])]
    action: String,
    #[arg(long)]
    review_id: Option<String>,
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
fn removal_review(diff: &str, layout: &[Vec<String>]) -> Result<Value> {
    let mut removals: Vec<_> = diff
        .lines()
        .filter(|line| line.starts_with("remove "))
        .map(str::to_owned)
        .collect();
    removals.sort();
    let removed = summary(diff, "removed")?;
    ensure!(
        removals.len() as u64 == removed,
        "Removal inventory does not match diff summary"
    );
    let id = crate::util::digest(&json!({"layout":layout,"removals":removals}));
    Ok(json!({"id":id,"created_at":now(),"removed":removed,"removals":removals}))
}
fn approval_matches(approval: &Value, review: &Value) -> bool {
    approval["review_id"] == review["id"]
        && approval["review_id"].is_string()
        && approval["approved_at"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .is_some_and(|t| {
                let age = chrono::Utc::now().signed_duration_since(t).num_seconds();
                (0..86400).contains(&age)
            })
}
fn checked_review_id(id: &str) -> Result<()> {
    ensure!(
        id.len() == 64
            && id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "Invalid review identifier"
    );
    Ok(())
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
                if removed > self.max_removed {
                    let review = removal_review(&diff, &self.layout)?;
                    atomic_json(&self.state.join("removal-review.json"), &review)?;
                    let approval_path = self.state.join("removal-approval.json");
                    let approval = if approval_path.exists() {
                        read_json(&approval_path)?
                    } else {
                        Value::Null
                    };
                    ensure!(
                        approval_matches(&approval, &review),
                        "{removed} removals exceed limit {}; review diff before any sync",
                        self.max_removed
                    );
                    if action == "sync" {
                        // Consume before parity changes; failures require fresh human approval.
                        remove(&approval_path)?;
                        atomic_json(
                            &self.state.join("last-removal-approval-used.json"),
                            &approval,
                        )?;
                    }
                }
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
                let log = self.log_dir.join(format!("{stamp}-sync.log"));
                run_maintenance(
                    r,
                    "sync",
                    &log,
                    Command::new("snapraid")
                        .arg("-c")
                        .arg(&self.snapraid_config)
                        .arg("-l")
                        .arg(&log)
                        .arg("sync"),
                )?;
            } else {
                let log = self.log_dir.join(format!("{stamp}-scrub.log"));
                run_maintenance(
                    r,
                    "scrub",
                    &log,
                    Command::new("snapraid")
                        .arg("-c")
                        .arg(&self.snapraid_config)
                        .arg("-l")
                        .arg(&log)
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
    if cli.action == "review" || cli.action == "approve-removals" {
        if cli.action == "review" && cli.review_id.as_deref() == Some("latest") {
            let mut logs = fs::read_dir(&p.log_dir)?
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().ends_with("-diff.log"))
                .map(|e| e.path())
                .collect::<Vec<_>>();
            logs.sort();
            let latest = logs.last().context("No completed diff to review")?;
            no_symlinks(latest)?;
            let diff = fs::read_to_string(latest)?;
            let review = removal_review(&diff, &p.layout)?;
            atomic_json(&p.state.join("removal-review.json"), &review)?;
            println!("{}", review["id"].as_str().unwrap());
            return Ok(());
        }
        let requested = cli
            .review_id
            .as_deref()
            .context("--review-id is required")?;
        checked_review_id(requested)?;
        let review = read_json(&p.state.join("removal-review.json"))?;
        ensure!(
            review["id"] == requested,
            "Removal list changed; open the current review instead"
        );
        if cli.action == "review" {
            println!("SnapRAID removal review\nReview: {requested}\nMissing files: {}\n\nThese files are already absent from the data disks. APPROVE & SYNC accepts\nthe listed removals into parity, reducing recovery options for these files.\nIt does not delete existing source files. Other sync safety gates still apply.\n\n", review["removed"]);
            for line in review["removals"]
                .as_array()
                .context("Missing removal list")?
            {
                println!("{}", line.as_str().context("Invalid removal record")?);
            }
            return Ok(());
        }
        ensure!(
            unsafe { libc::geteuid() } == 0,
            "Removal approval requires root"
        );
        atomic_json(
            &p.state.join("removal-approval.json"),
            &json!({"review_id":requested,"approved_at":now()}),
        )?;
        let unit = format!("synctjik-reviewed-sync-{}", &requested[..16]);
        r.run(
            Command::new("systemd-run")
                .args([
                    "--no-block",
                    "--collect",
                    "--unit",
                    &unit,
                    "--property=Type=oneshot",
                    "--property=TimeoutStartSec=infinity",
                    "--property=After=si-snapraid-scrub.service si-snapraid-sync.service",
                ])
                .arg(std::env::current_exe()?)
                .args(["snapraid", "sync", "--policy"])
                .arg(&cli.policy),
        )?;
        println!("Approved this exact removal list for one sync within 24 hours; sync queued behind maintenance.");
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

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn failure_reports_counts_without_exposing_filenames() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("scrub.log");
        fs::write(&log, "msg:error: Missing file '/private/secret'.\nmsg:error: Missing file '/private/secret'.\nsummary:error_soft:2\nsummary:error_io:0\nsummary:error_data:0\n").unwrap();
        let message = command_failure("scrub", 1, &log);
        assert!(message.contains("SnapRAID scrub failed (exit 1): 2 soft errors (1 missing files), 0 I/O errors, 0 data errors"));
        assert!(message.contains(log.to_str().unwrap()));
        assert!(!message.contains("secret"));
        fs::write(
            &log,
            "summary:error_soft:2\nsummary:error_io:1\nsummary:error_data:3\n",
        )
        .unwrap();
        let message = command_failure("scrub", 1, &log);
        assert!(message.contains("2 soft errors, 1 I/O errors, 3 data errors"));
        assert!(!message.contains("missing files"));
    }
    #[test]
    fn failure_handles_missing_or_invalid_summary_without_claiming_clean_data() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("sync.log");
        for contents in [None, Some("summary:error_soft:private-name\nsummary:error_io:0\nsummary:error_data:0\n"), Some("summary:error_soft:0\nsummary:error_soft:1\nsummary:error_io:0\nsummary:error_data:0\n")] {
            if let Some(contents) = contents { fs::write(&log, contents).unwrap(); }
            assert_eq!(command_failure("sync", 2, &log), format!("SnapRAID sync failed (exit 2). Log: {}", log.display()));
        }
    }
    #[test]
    fn failed_process_uses_snapraid_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("scrub.log");
        let result = run_maintenance(
            &Runner::new().unwrap(),
            "scrub",
            &log,
            Command::new("sh").args(["-c", "exit 1"]),
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .starts_with("SnapRAID scrub failed (exit 1). Log:"));
    }
    #[test]
    fn approval_is_bound_to_exact_removals_layout_and_age() {
        let layout = vec![vec!["data".into(), "d1".into(), "/disk".into()]];
        let a = removal_review("remove d1/a\nremove d1/b\n2 removed\n", &layout).unwrap();
        let reordered = removal_review("remove d1/b\nremove d1/a\n2 removed\n", &layout).unwrap();
        assert_eq!(a["id"], reordered["id"]);
        let approval = json!({"review_id":a["id"],"approved_at":now()});
        assert!(approval_matches(&approval, &a));
        let b = removal_review("remove d1/a\nremove d1/c\n2 removed\n", &layout).unwrap();
        assert!(!approval_matches(&approval, &b));
        assert!(!approval_matches(
            &json!({"review_id":a["id"],"approved_at":"2000-01-01T00:00:00Z"}),
            &a
        ));
        assert!(removal_review("remove d1/a\n2 removed\n", &layout).is_err());
        assert!(checked_review_id("../../bad").is_err());
        let changed = removal_review("remove d1/a\nremove d1/b\n2 removed\n", &[]).unwrap();
        assert!(!approval_matches(&approval, &changed));
    }
}
