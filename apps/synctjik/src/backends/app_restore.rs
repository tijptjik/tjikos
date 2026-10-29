use super::{app_backup::Policy, process::*, protection::*};
use crate::util::{atomic_json, hash};
use anyhow::{ensure, Context, Result};
use clap::Parser;
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Parser)]
#[command(
    name = "synctjik app-restore",
    about = "Verify an exact application snapshot, then explicitly restore into empty destinations"
)]
struct Cli {
    #[arg(value_parser=["stage", "install", "import-mariadb", "verify"])]
    action: String,
    #[arg(long, default_value = "/etc/si-app-backup/policy.json")]
    policy: PathBuf,
    #[arg(long)]
    stage: PathBuf,
    #[arg(long)]
    repository: Option<String>,
    #[arg(long)]
    password_file: Option<PathBuf>,
    #[arg(long)]
    snapshot: Option<String>,
    #[arg(long)]
    include: Vec<PathBuf>,
}
fn root_for(stage: &Path) -> Result<PathBuf> {
    no_symlinks(stage)?;
    let manifests: Vec<_> = walk(stage)?
        .into_iter()
        .filter(|p| p.file_name().is_some_and(|n| n == "backup-manifest.json"))
        .collect();
    ensure!(
        manifests.len() == 1,
        "Expected exactly one backup manifest in staging"
    );
    Ok(manifests[0]
        .parent()
        .context("Missing manifest parent")?
        .into())
}
fn quiet(p: &Policy, r: &Runner, mariadb: bool) -> Result<()> {
    if status(
        r,
        Command::new("systemctl").args(["is-active", "--quiet", "docker"]),
    )?
    .0 == 0
    {
        let names = text(
            r,
            Command::new("docker").args(["ps", "--format", "{{.Names}}"]),
        )?;
        ensure!(
            names
                .split_whitespace()
                .all(|n| mariadb && n == p.mariadb.container),
            "Stop containers explicitly before restoring"
        );
    }
    for service in &p.quiet_services {
        ensure!(
            status(
                r,
                Command::new("systemctl").args(["is-active", "--quiet", service])
            )?
            .0 != 0,
            "Stop application services explicitly before restoring"
        );
    }
    Ok(())
}
pub fn main(args: &[String], r: &Runner) -> Result<()> {
    let cli = Cli::parse_from(
        std::iter::once("synctjik app-restore".to_string()).chain(args.iter().cloned()),
    );
    let p: Policy = policy(&cli.policy)?;
    p.validate()?;
    // Recovery staging must also work on a replacement/offsite computer.
    // Live installation remains gated by disk identities and stopped writers.
    p.user()?;
    no_symlinks(&cli.stage)?;
    let _lock = Lock::take(&p.lock, false, r)?;
    if cli.action == "stage" {
        let id = cli.snapshot.context("Supply --snapshot")?;
        exact_snapshot(&id)?;
        let repo = cli.repository.context("Supply --repository")?;
        let password = cli.password_file.context("Supply --password-file")?;
        secret(&password)?;
        new_target(&cli.stage)?;
        r.run(
            Command::new("restic")
                .env("RESTIC_REPOSITORY", repo)
                .env("RESTIC_PASSWORD_FILE", password)
                .env("RESTIC_CACHE_DIR", &p.restic.cache)
                .args([
                    "--retry-lock",
                    "15m",
                    "restore",
                    &id,
                    "--verify",
                    "--target",
                ])
                .arg(&cli.stage),
        )?;
        p.validate_restore(&root_for(&cli.stage)?, r)?;
        println!("Staged and verified. No live configuration changed.");
        return Ok(());
    }
    let root = root_for(&cli.stage)?;
    p.validate_restore(&root, r)?;
    if cli.action == "verify" {
        println!("Restored application databases and manifest verified");
        return Ok(());
    }
    p.restore_mounts.check(r)?;
    p.guard(r)?;
    quiet(&p, r, cli.action == "import-mariadb")?;
    if cli.action == "install" {
        let selected = if cli.include.is_empty() {
            &p.restore_defaults
        } else {
            &cli.include
        };
        ensure!(!selected.is_empty(), "No restore destinations selected");
        for target in selected {
            ensure!(
                p.restore_defaults.contains(target) || p.restore_extra.contains(target),
                "Path is outside restore allowlist"
            );
            let source = root.join(target.strip_prefix("/")?);
            no_symlinks(&source)?;
            ensure!(source.is_dir(), "Missing restore source");
            empty(target)?;
            ensure!(
                !cli.stage.starts_with(target) && !target.starts_with(&cli.stage),
                "Restore destination overlaps staging"
            );
        }
        for target in selected {
            empty(target)?;
            fs::create_dir_all(target)?;
            r.run(
                Command::new("rsync")
                    .args(["-aHAX", "--numeric-ids", "--ignore-existing"])
                    .arg(format!(
                        "{}/{}/",
                        root.display(),
                        target.strip_prefix("/")?.display()
                    ))
                    .arg(format!("{}/", target.display())),
            )?;
        }
        println!("Selected state restored. Services remain stopped. MariaDB import is separate.");
    } else {
        let inspected: Vec<Value> = serde_json::from_slice(
            &r.run(Command::new("docker").args(["inspect", &p.mariadb.container]))?,
        )?;
        ensure!(
            inspected.len() == 1
                && inspected[0]["Mounts"]
                    .as_array()
                    .context("Missing container mounts")?
                    .iter()
                    .any(|m| m["Source"] == path(&p.mariadb.config_source)
                        && m["Destination"] == p.mariadb.config_target),
            "Unexpected MariaDB target mount"
        );
        let count = text(r, Command::new("docker").args(["exec", &p.mariadb.container, "mariadb", "-uroot", "-Nse", "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema NOT IN ('mysql','information_schema','performance_schema','sys');"]))?;
        ensure!(
            count == "0",
            "MariaDB contains application tables; refusing replacement"
        );
        ensure!(
            !p.mariadb.receipt.exists(),
            "MariaDB restore receipt exists; refusing repeated import"
        );
        let dump = root.join("database-dumps/mariadb.sql");
        let sha = hash(&dump)?;
        atomic_json(
            &p.mariadb.receipt,
            &json!({"sha256":sha, "status":"started"}),
        )?;
        r.run(
            Command::new("docker")
                .args(["exec", "-i", &p.mariadb.container, "mariadb", "-uroot"])
                .stdin(File::open(&dump)?),
        )?;
        atomic_json(
            &p.mariadb.receipt,
            &json!({"sha256":sha, "status":"complete"}),
        )?;
        println!("MariaDB imported into checked fresh instance. Other services remain stopped.");
    }
    Ok(())
}
