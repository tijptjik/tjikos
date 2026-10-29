use super::{process::*, protection::*};
use crate::util::{atomic_json, now, private_dir, read_json};
use anyhow::{ensure, Context, Result};
use clap::Parser;
use rusqlite::{
    backup::{Backup, StepResult},
    Connection, OpenFlags,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    io::Read,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    name = "synctjik app-backup",
    about = "Consistent application snapshots with SQLite and MariaDB exports"
)]
struct Cli {
    #[arg(default_value="backup", value_parser=["backup", "check"])]
    action: String,
    #[arg(long, default_value = "/etc/si-app-backup/policy.json")]
    policy: PathBuf,
}
#[derive(Deserialize)]
pub(super) struct Repair {
    pub path: PathBuf,
    pub checks: Vec<String>,
    pub evidence: String,
    pub label: String,
}
#[derive(Deserialize)]
pub(super) struct Maria {
    pub container: String,
    pub config_source: PathBuf,
    pub config_target: String,
    pub receipt: PathBuf,
}
#[derive(Deserialize)]
pub(super) struct Policy {
    pub host: String,
    pub required_uid: Option<u32>,
    pub state: PathBuf,
    pub lock: PathBuf,
    pub stage: PathBuf,
    pub restic: Restic,
    pub snapraid_config: PathBuf,
    pub sources: Vec<(PathBuf, Vec<String>)>,
    pub direct_sources: Vec<PathBuf>,
    pub plex_root: PathBuf,
    pub plex_sqlite: PathBuf,
    #[serde(default)]
    pub repairs: Vec<Repair>,
    pub mariadb: Maria,
    pub restore_defaults: Vec<PathBuf>,
    pub restore_extra: Vec<PathBuf>,
    pub quiet_services: Vec<String>,
    pub restore_mounts: Mounts,
    #[serde(flatten)]
    pub mounts: Mounts,
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        paths(&[
            &self.state,
            &self.lock,
            &self.stage,
            &self.restic.repository,
            &self.mariadb.receipt,
        ])?;
        self.restic.validate()?;
        ensure!(
            !self.sources.is_empty() && !self.direct_sources.is_empty(),
            "Application source selection is empty"
        );
        ensure!(
            !self.stage.starts_with(&self.restic.repository)
                && !self.restic.repository.starts_with(&self.stage),
            "Staging overlaps repository"
        );
        for (source, _) in &self.sources {
            absolute(source)?;
            ensure!(
                !self.stage.starts_with(source) && !source.starts_with(&self.stage),
                "Staging overlaps source"
            );
        }
        for p in &self.direct_sources {
            absolute(p)?;
        }
        for p in self.restore_defaults.iter().chain(&self.restore_extra) {
            absolute(p)?;
            ensure!(p != Path::new("/"), "Cannot restore root");
        }
        for repair in &self.repairs {
            crate::util::host(&repair.evidence)?;
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<()> {
        host_guard(&self.host)?;
        self.user()
    }
    pub fn user(&self) -> Result<()> {
        if let Some(uid) = self.required_uid {
            ensure!(
                unsafe { libc::geteuid() } == uid,
                "Policy requires a different effective user"
            );
        }
        Ok(())
    }
    pub fn guard(&self, r: &Runner) -> Result<()> {
        self.mounts.check(r)?;
        for line in fs::read_to_string(&self.snapraid_config)?.lines() {
            let words: Vec<_> = line.split_whitespace().collect();
            if words.first() == Some(&"data") {
                ensure!(
                    words.len() >= 3 && mounted(Path::new(words[2])),
                    "Data disk is not mounted"
                );
            }
        }
        Ok(())
    }
    fn historical(&self, source: &Path) -> bool {
        source.parent() == Some(self.plex_root.join("Plug-in Support/Databases").as_path())
            && path(source).ends_with(".db.bak")
    }
    fn plex_check(&self, r: &Runner, db: &Path) -> Result<Vec<String>> {
        Ok(text(
            r,
            Command::new(&self.plex_sqlite)
                .arg("-readonly")
                .arg(db)
                .arg("PRAGMA quick_check;"),
        )?
        .lines()
        .map(String::from)
        .collect())
    }
    fn stage_files(&self, r: &Runner) -> Result<()> {
        for (src, excludes) in &self.sources {
            ensure!(src.is_dir(), "Required application source missing");
            let dest = self.stage.join(src.strip_prefix("/")?);
            no_symlinks(&dest)?;
            fs::create_dir_all(&dest)?;
            let mut cmd = Command::new("rsync");
            cmd.args(["-aHAX", "--numeric-ids", "--delete", "--delete-excluded"]);
            for exclude in excludes {
                cmd.arg("--exclude").arg(exclude);
            }
            cmd.arg(format!("{}/", src.display()))
                .arg(format!("{}/", dest.display()));
            let (code, _) = status(r, &mut cmd)?;
            ensure!(
                code == 0 || code == 24,
                "Application staging failed (exit {code})"
            );
        }
        Ok(())
    }
    fn databases(&self, r: &Runner) -> Result<(Vec<Value>, Vec<Value>)> {
        let mut active = Vec::new();
        let mut historical = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (root, _) in &self.sources {
            for staged in walk(&self.stage.join(root.strip_prefix("/")?))? {
                if !seen.insert(staged.clone()) {
                    continue;
                }
                let mut header = [0; 16];
                if File::open(&staged)?.read(&mut header)? != 16 || &header != b"SQLite format 3\0"
                {
                    continue;
                }
                let source = Path::new("/").join(staged.strip_prefix(&self.stage)?);
                ensure!(source.is_file(), "Database disappeared during backup");
                if self.historical(&source) {
                    historical.push(json!({"path":source, "quickCheck":self.plex_check(r, &staged)?, "bytes":staged.metadata()?.len(), "role":"historical Plex database backup; not a live restore database"}));
                    continue;
                }
                let tmp = staged.with_file_name(format!(
                    "{}.backup-tmp",
                    staged
                        .file_name()
                        .context("Missing filename")?
                        .to_string_lossy()
                ));
                no_symlinks(&tmp)?;
                remove(&tmp)?;
                let metadata = source.metadata()?;
                let src = Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                src.busy_timeout(Duration::from_secs(30))?;
                let mut dst = Connection::open(&tmp)?;
                online_backup(r, &src, &mut dst)?;
                let checks = quick_check(&dst, false);
                let mut deferred = false;
                let mut checks = match checks {
                    Ok(c) => c,
                    Err(_) if source.starts_with(&self.plex_root) => {
                        deferred = true;
                        vec![]
                    }
                    Err(e) => return Err(e),
                };
                let mut repaired = Vec::new();
                if let Some(repair) = self
                    .repairs
                    .iter()
                    .find(|p| p.path == source && p.checks == checks)
                {
                    let evidence = self.stage.join("database-evidence");
                    private_dir(&evidence)?;
                    let original = evidence.join(&repair.evidence);
                    no_symlinks(&original)?;
                    remove(&original)?;
                    let mut preserved = Connection::open(&original)?;
                    online_backup(r, &dst, &mut preserved)?;
                    drop(preserved);
                    dst.execute_batch("REINDEX;")?;
                    checks = quick_check(&dst, true)?;
                    repaired.push(repair.label.clone());
                }
                drop(dst);
                drop(src);
                if deferred {
                    checks = self.plex_check(r, &tmp)?;
                }
                ensure!(checks == ["ok"], "Database integrity failure");
                fs::set_permissions(&tmp, fs::Permissions::from_mode(metadata.mode() & 0o7777))?;
                let f = File::open(&tmp)?;
                ensure!(
                    unsafe { libc::fchown(f.as_raw_fd(), metadata.uid(), metadata.gid()) } == 0,
                    "Cannot preserve database ownership"
                );
                f.sync_all()?;
                fs::rename(&tmp, &staged)?;
                for suffix in ["-wal", "-shm", "-journal"] {
                    remove(&PathBuf::from(format!("{}{suffix}", staged.display())))?;
                }
                active.push(json!({"path":source, "quickCheck":"ok", "rebuiltIndexesInBackupOnly":repaired, "bytes":staged.metadata()?.len()}));
            }
        }
        ensure!(
            !active.is_empty(),
            "No validated SQLite databases; refusing incomplete application snapshot"
        );
        Ok((active, historical))
    }
    fn backup(&self, r: &Runner) -> Result<()> {
        self.identity()?;
        self.guard(r)?;
        for p in &self.direct_sources {
            ensure!(p.is_dir(), "Required direct source missing");
        }
        for (p, _) in &self.sources {
            ensure!(p.is_dir(), "Required source missing");
        }
        self.restic.init(r, &[])?;
        private_dir(&self.stage)?;
        // Incomplete staging must never retain a prior run's completion manifest.
        remove(&self.stage.join("backup-manifest.json"))?;
        self.stage_files(r)?;
        let started = now();
        let (sqlite, historical) = self.databases(r)?;
        let dumps = self.stage.join("database-dumps");
        private_dir(&dumps)?;
        let mut tmp = tempfile::NamedTempFile::new_in(&dumps)?;
        r.run_to(
            Command::new("docker").args([
                "exec",
                &self.mariadb.container,
                "mariadb-dump",
                "-uroot",
                "--all-databases",
                "--lock-all-tables",
                "--routines",
                "--events",
                "--triggers",
                "--hex-blob",
            ]),
            tmp.as_file_mut(),
        )?;
        ensure!(
            tmp.as_file().metadata()?.len() >= 1000,
            "Missing or empty MariaDB dump"
        );
        tmp.persist(dumps.join("mariadb.sql"))?;
        let containers = text(
            r,
            Command::new("docker").args(["ps", "--format", "{{.Names}}"]),
        )?;
        atomic_json(
            &self.stage.join("backup-manifest.json"),
            &json!({
                "capturedAt":started, "sources":self.sources, "directSources":self.direct_sources,
                "contentVerification":"Restic --force rereads all source bytes and deduplicates content chunks on every backup",
                "runningContainers":containers.lines().collect::<Vec<_>>(),
                "consistency":"SQLite online backup API with quick_check; MariaDB locked logical dump; application configuration copied with rsync",
                "sqlite":sqlite, "historicalSqlite":historical, "mariadbDump":"database-dumps/mariadb.sql",
                "mariadbRestore":"Import with compatible MariaDB; raw databases directory intentionally excluded"
            }),
        )?;
        self.guard(r)?;
        r.run(
            self.restic
                .command()
                .args(["backup", "--force"])
                .arg(&self.stage)
                .args(&self.direct_sources)
                .args(["--host", &self.restic.host, "--tag", &self.restic.tag]),
        )?;
        self.restic.retain(r, &self.restic.tag)?;
        r.run(self.restic.command().arg("check"))?;
        atomic_json(
            &self.state.join("last-success.json"),
            &json!({"completedAt":now(), "repository":self.restic.repository}),
        )?;
        println!("BACKUP SUCCESS");
        Ok(())
    }
    pub fn validate_restore(&self, root: &Path, r: &Runner) -> Result<()> {
        let manifest = read_json(&root.join("backup-manifest.json"))?;
        let records = manifest["sqlite"]
            .as_array()
            .context("Unsupported backup manifest")?;
        ensure!(
            !records.is_empty() && manifest["mariadbDump"] == "database-dumps/mariadb.sql",
            "Unsupported backup manifest"
        );
        for record in records {
            let original = Path::new(record["path"].as_str().context("Invalid database path")?);
            absolute(original)?;
            let db = root.join(original.strip_prefix("/")?);
            no_symlinks(&db)?;
            ensure!(db.is_file(), "Missing database in restore");
            let check = if original.starts_with(&self.plex_root) {
                self.plex_check(r, &db)?
            } else {
                quick_check(
                    &Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?,
                    false,
                )?
            };
            ensure!(check == ["ok"], "Restored database integrity failure");
        }
        let dump = root.join("database-dumps/mariadb.sql");
        no_symlinks(&dump)?;
        ensure!(
            dump.is_file() && dump.metadata()?.len() >= 1000,
            "Missing or empty MariaDB dump"
        );
        Ok(())
    }
}
fn online_backup(r: &Runner, source: &Connection, target: &mut Connection) -> Result<()> {
    let backup = Backup::new(source, target)?;
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        r.check()?;
        ensure!(Instant::now() < deadline, "SQLite backup timed out");
        match backup.step(1024)? {
            StepResult::Done => break,
            StepResult::Busy | StepResult::Locked => std::thread::sleep(Duration::from_millis(50)),
            _ => (),
        }
    }
    Ok(())
}
fn quick_check(c: &Connection, full: bool) -> Result<Vec<String>> {
    let mut stmt = c.prepare(if full {
        "PRAGMA integrity_check"
    } else {
        "PRAGMA quick_check"
    })?;
    let rows = stmt
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(rows)
}
pub fn main(args: &[String], r: &Runner) -> Result<()> {
    let cli = Cli::parse_from(
        std::iter::once("synctjik app-backup".to_string()).chain(args.iter().cloned()),
    );
    let p: Policy = policy(&cli.policy)?;
    p.validate()?;
    if cli.action == "check" {
        println!("Application backup policy valid");
        return Ok(());
    }
    private_dir(&p.state)?;
    let _lock = Lock::take(&p.lock, true, r)?;
    let result = p.backup(r);
    record_error(&p.state, &result)?;
    result
}
