use super::{process::*, protection::*};
use crate::util::{atomic_json, now, private_dir};
use anyhow::{ensure, Context, Result};
use clap::Parser;
use serde::Deserialize;
use serde_json::json;
use std::{
    fs,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    name = "synctjik drive",
    about = "Export selected Drive trees to independently encrypted Restic snapshots"
)]
struct Cli {
    #[arg(default_value="backup", value_parser=["backup", "check", "snapshots", "restore"])]
    action: String,
    #[arg(long)]
    policy: Option<PathBuf>,
    #[arg(long)]
    snapshot: Option<String>,
    #[arg(long)]
    to: Option<PathBuf>,
    #[arg(long)]
    tag: Option<String>,
}
#[derive(Deserialize)]
struct Source {
    tag: String,
    source: String,
    name: String,
}
#[derive(Deserialize)]
struct Policy {
    host: String,
    state: PathBuf,
    mount_root: PathBuf,
    rclone_config: PathBuf,
    export_formats: String,
    sources: Vec<Source>,
    restic: Restic,
    #[serde(flatten)]
    mounts: Mounts,
}
impl Policy {
    fn validate(&self) -> Result<()> {
        paths(&[&self.state, &self.mount_root, &self.rclone_config])?;
        self.restic.validate()?;
        ensure!(
            !self.sources.is_empty() && !self.export_formats.is_empty(),
            "No Drive sources or export formats"
        );
        let mut names = std::collections::HashSet::new();
        let mut tags = std::collections::HashSet::new();
        for s in &self.sources {
            crate::util::host(&s.name)?;
            ensure!(
                names.insert(&s.name)
                    && tags.insert(&s.tag)
                    && !s.tag.is_empty()
                    && s.source.contains(':'),
                "Invalid or duplicate Drive source"
            );
        }
        Ok(())
    }
    fn rclone(&self) -> Command {
        let mut c = Command::new("rclone");
        c.arg("--config")
            .arg(&self.rclone_config)
            .arg("--drive-export-formats")
            .arg(&self.export_formats);
        c
    }
    fn mount_backup(&self, r: &Runner, s: &Source) -> Result<()> {
        let target = self.mount_root.join(&s.name);
        no_symlinks(&target)?;
        if mounted(&target) {
            let kind = text(
                r,
                Command::new("findmnt")
                    .args(["--noheadings", "--raw", "--mountpoint"])
                    .arg(&target)
                    .args(["--output", "FSTYPE"]),
            )?;
            ensure!(
                kind == "fuse.rclone",
                "Unexpected filesystem at Drive mount"
            );
            r.run(Command::new("fusermount3").arg("-u").arg(&target))?;
        }
        empty(&target)?;
        private_dir(&target)?;
        let mut child = ManagedChild(
            self.rclone()
                .args(["mount", "--read-only", "--vfs-cache-mode", "off", &s.source])
                .arg(&target)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(tempfile::tempfile()?)
                .process_group(0)
                .spawn()?,
            true,
        );
        let result = (|| {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                r.check()?;
                ensure!(
                    child.0.try_wait()?.is_none(),
                    "Drive mount exited before backup finished"
                );
                if mounted(&target) {
                    break;
                }
                ensure!(
                    Instant::now() < deadline,
                    "Timed out waiting for Drive mount"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
            ensure!(
                fs::read_dir(&target)?.next().is_some(),
                "Mounted Drive source is empty"
            );
            self.mounts.check(r)?;
            r.guarded(
                self.restic
                    .command()
                    .args([
                        "backup",
                        "--host",
                        &self.restic.host,
                        "--tag",
                        &self.restic.tag,
                        "--tag",
                        &s.tag,
                    ])
                    .arg(&target),
                Some(&mut child.0),
                false,
            )?;
            Ok(())
        })();
        // Cancellation must not prevent releasing the FUSE mount.
        let cleanup = Runner::new()?;
        if mounted(&target) {
            let _ = status(&cleanup, Command::new("fusermount3").arg("-u").arg(&target));
        }
        drop(child);
        if mounted(&target) {
            let _ = status(
                &cleanup,
                Command::new("fusermount3").arg("-uz").arg(&target),
            );
        }
        ensure!(!mounted(&target), "Could not unmount Drive source");
        fs::remove_dir(&target)?;
        result
    }
    fn backup(&self, r: &Runner) -> Result<()> {
        host_guard(&self.host)?;
        self.mounts.check(r)?;
        secret(&self.rclone_config)?;
        secret(&self.restic.password)?;
        private_dir(&self.mount_root)?;
        self.restic.init(r, &[])?;
        let mut receipts = Vec::new();
        for s in &self.sources {
            let listing = text(
                r,
                self.rclone().args(["lsf", "--max-depth", "1", &s.source]),
            )?;
            let count = listing.lines().filter(|l| !l.is_empty()).count();
            ensure!(count > 0, "Drive source is empty; refusing backup");
            self.mount_backup(r, s)?;
            receipts.push(json!({"tag":s.tag, "source":s.source, "top_level_entries":count, "google_native_export_formats":self.export_formats}));
        }
        self.mounts.check(r)?;
        r.run(self.restic.command().arg("check"))?;
        for s in &self.sources {
            self.restic.retain(r, &s.tag)?;
        }
        atomic_json(
            &self.state.join("last-success.json"),
            &json!({"completed":now(), "repository":self.restic.repository, "sources":receipts}),
        )?;
        println!("DONE Drive exports backed up, checked, and retained");
        Ok(())
    }
}
pub fn main(args: &[String], r: &Runner) -> Result<()> {
    let cli =
        Cli::parse_from(std::iter::once("synctjik drive".to_string()).chain(args.iter().cloned()));
    let p: Policy = policy(
        &cli.policy
            .unwrap_or(home()?.join(".config/synctjik/drive.json")),
    )?;
    p.validate()?;
    if cli.action == "check" {
        println!("Drive policy valid");
        return Ok(());
    }
    private_dir(&p.state)?;
    let _lock = Lock::take(&p.state.join("operation.lock"), false, r)?;
    let result = match cli.action.as_str() {
        "backup" => p.backup(r),
        "snapshots" | "restore" => (|| {
            // A verified repository may be recovered on a replacement host.
            p.mounts.check(r)?;
            secret(&p.restic.password)?;
            let tag = cli.tag.context("Supply --tag")?;
            ensure!(
                p.sources.iter().any(|s| s.tag == tag),
                "Unknown Drive source tag"
            );
            if cli.action == "snapshots" {
                println!(
                    "{}",
                    text(
                        r,
                        p.restic.command().args([
                            "snapshots",
                            "--host",
                            &p.restic.host,
                            "--tag",
                            &tag,
                            "--json"
                        ])
                    )?
                );
            } else {
                let id = cli.snapshot.context("Supply --snapshot")?;
                exact_snapshot(&id)?;
                let snapshots: Vec<serde_json::Value> =
                    serde_json::from_slice(&r.run(p.restic.command().args([
                        "snapshots",
                        "--host",
                        &p.restic.host,
                        "--tag",
                        &tag,
                        "--json",
                    ]))?)?;
                ensure!(
                    snapshots.iter().any(|s| s["id"] == id),
                    "Snapshot does not belong to selected source"
                );
                let target = cli.to.context("Supply --to")?;
                new_target(&target)?;
                r.run(
                    p.restic
                        .command()
                        .args(["restore", &id, "--verify", "--target"])
                        .arg(&target),
                )?;
                println!("Drive snapshot restored and verified");
            }
            Ok(())
        })(),
        _ => unreachable!(),
    };
    record_error(&p.state, &result)?;
    result
}
