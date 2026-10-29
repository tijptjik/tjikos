use super::process::*;
use crate::util::{atomic_json, private_dir, read_json};
use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Datelike, Local, NaiveDateTime, Utc};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    name = "synctjik private-assets",
    about = "Local-first private assets, verified backups and explicit mounts"
)]
struct Cli {
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    Sync {
        folders: Vec<String>,
        #[arg(long)]
        progress: bool,
        #[arg(long, default_value_t=5, value_parser=clap::value_parser!(u8).range(5..=100))]
        max_delete_percent: u8,
    },
    Init {
        folders: Vec<String>,
    },
    Backup {
        #[arg(long)]
        scheduled: bool,
    },
    Snapshots {
        #[arg(long="from", value_parser=["fi","li"])]
        source: Option<String>,
    },
    Restore {
        #[arg(long="from", value_parser=["fi","li"])]
        source: String,
        #[arg(long, default_value = "latest")]
        snapshot: String,
        #[arg(long)]
        to: PathBuf,
    },
    Status,
    Ready,
    CryptPassword,
    Mount {
        #[arg(value_parser=["crypt","giprivate","gipublic","siprivate","sipublic"])]
        which: String,
    },
}
#[derive(Deserialize)]
struct Policy {
    sharing_approved: bool,
    folders: Vec<String>,
    remote: String,
    backup_root: String,
    #[serde(default = "recovery_days")]
    recovery_days: i64,
    crypt_secret_id: String,
    ssh_command: Vec<String>,
    ssh_hosts: Vec<String>,
    ssh_hostname: String,
    drive_remote: String,
    server_mount_root: String,
}
fn recovery_days() -> i64 {
    31
}
struct App<'a> {
    home: PathBuf,
    config: PathBuf,
    state: PathBuf,
    host: String,
    p: Policy,
    runner: &'a Runner,
}
fn stamp() -> String {
    Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}
fn leaf(s: &str) -> Result<()> {
    ensure!(
        !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\0', '\n', '\r']),
        "Invalid folder name"
    );
    Ok(())
}
impl<'a> App<'a> {
    fn load(runner: &'a Runner) -> Result<Self> {
        let home = home()?;
        let config = home.join(".config/private-storage");
        let p: Policy = serde_json::from_value(read_json(&config.join("config.json"))?)
            .context("Invalid private-assets policy")?;
        ensure!(
            !p.folders.is_empty()
                && !p.ssh_command.is_empty()
                && !p.ssh_hosts.is_empty()
                && p.recovery_days > 0,
            "Incomplete private-assets policy"
        );
        for folder in &p.folders {
            leaf(folder)?;
        }
        absolute(Path::new(&p.backup_root))?;
        let bytes = runner.run(&mut Command::new("hostname"))?;
        let host = String::from_utf8(bytes)?
            .trim()
            .split('.')
            .next()
            .context("Hostname unavailable")?
            .to_owned();
        crate::util::host(&host)?;
        Ok(Self {
            state: home.join(".local/state/private-storage"),
            home,
            config,
            host,
            p,
            runner,
        })
    }
    fn rclone(&self) -> Vec<String> {
        vec![
            "rclone".into(),
            "--config".into(),
            path(&self.config.join("drive.conf")),
        ]
    }
    fn run(&self, args: &[String]) -> Result<Vec<u8>> {
        self.runner.run(&mut command(args))
    }
    fn ssh(&self) -> Result<Vec<String>> {
        for host in &self.p.ssh_hosts {
            let mut args = self.p.ssh_command.clone();
            args.extend([host.clone(), "hostname".into()]);
            let (code, bytes) = self.runner.guarded(&mut command(&args), None, true)?;
            if code == 0 && String::from_utf8_lossy(&bytes).trim() == self.p.ssh_hostname {
                args.pop();
                return Ok(args);
            }
        }
        anyhow::bail!("Backup server is unreachable with its verified SSH host key")
    }
    fn restic(&self, source: &str) -> Result<(Vec<String>, Vec<String>, String)> {
        ensure!(
            ["fi", "li"].contains(&source),
            "Choose backup source fi or li"
        );
        let ssh = self.ssh()?;
        let root = format!("{}/{source}", self.p.backup_root.trim_end_matches('/'));
        let mut transport = ssh[..ssh.len() - 1].to_vec();
        transport.extend(["-s".into(), ssh.last().unwrap().clone(), "sftp".into()]);
        Ok((
            vec![
                "restic".into(),
                "-r".into(),
                format!("sftp:si:{root}"),
                "-p".into(),
                path(&self.config.join("restic-password")),
                "-o".into(),
                format!("sftp.command={}", shell_join(&transport)),
                "--retry-lock".into(),
                "5m".into(),
            ],
            ssh,
            root,
        ))
    }
    fn notify(&self, message: &str) {
        if available("notify-send") {
            let _ = self.run(&strings(&[
                "notify-send",
                "-a",
                "Synctjik",
                "Synctjik",
                message,
            ]));
        }
    }
    fn sync(
        &self,
        initial: bool,
        selected: &[String],
        progress: bool,
        max_delete: u8,
    ) -> Result<()> {
        ensure!(
            self.p.sharing_approved,
            "Drive sharing has not been approved; synchronization blocked"
        );
        ensure!(
            !initial || self.config.join("layout-ready").exists(),
            "Drive layout has not been verified; initialization blocked"
        );
        ensure!(
            selected.iter().all(|f| self.p.folders.contains(f)),
            "Unknown sync folder"
        );
        ensure!(
            max_delete <= 5 || (!initial && selected.len() == 1),
            "Delete override requires exactly one reviewed folder"
        );
        let folders = if selected.is_empty() {
            &self.p.folders
        } else {
            selected
        };
        for folder in folders {
            let local = self.home.join(folder);
            no_symlinks(&local)?;
            ensure!(local.is_dir(), "Expected a real local sync directory");
            let remote = format!("{}/{folder}", self.p.remote);
            let work = self.state.join("bisync").join(folder);
            no_symlinks(&work)?;
            let receipt = work.join("initialized.json");
            ensure!(
                initial || receipt.exists(),
                "{folder} has not been initialized; run synctjik private-assets init {folder}"
            );
            if receipt.exists() {
                let old = read_json(&receipt)?;
                ensure!(
                    old["host"] == self.host && old["remote"] == remote,
                    "Sync baseline belongs to another client or destination; review required"
                );
                if initial {
                    println!("– {folder}: already initialized");
                    continue;
                }
            }
            private_dir(&work)?;
            let logdir = self.state.join("logs");
            private_dir(&logdir)?;
            let when = stamp();
            let log = logdir.join(format!("{when}-{folder}.log"));
            let recovery = self.state.join("recovery").join(folder).join(&when);
            no_symlinks(&recovery)?;
            private_dir(&recovery)?;
            let mut args = self.rclone();
            args.extend(vec![
                "bisync".into(),
                path(&local),
                remote.clone(),
                "--workdir".into(),
                path(&work),
                "--filters-file".into(),
                path(&self.config.join("sync-filter.txt")),
                "--drive-skip-gdocs".into(),
                "--skip-links".into(),
                "--check-access".into(),
                "--check-filename".into(),
                ".private-sync-access".into(),
                "--max-delete".into(),
                max_delete.to_string(),
                "--max-delete-size".into(),
                "1Gi".into(),
                "--conflict-resolve".into(),
                "none".into(),
                "--conflict-loser".into(),
                "num".into(),
                "--conflict-suffix".into(),
                format!("conflict-{}", self.host),
                "--recover".into(),
                "--resilient".into(),
                "--backup-dir1".into(),
                path(&recovery),
                "--backup-dir2".into(),
                format!(
                    "{}/backups/sync-recovery/{}/{folder}/{when}",
                    self.p.remote, self.host
                ),
                "--create-empty-src-dirs".into(),
                "--log-level".into(),
                "INFO".into(),
                "--log-file".into(),
                path(&log),
            ]);
            if initial {
                let marker = local.join(".private-sync-access");
                no_symlinks(&marker)?;
                if !marker.exists() {
                    fs::write(
                        &marker,
                        "Private storage availability check. Do not delete.\n",
                    )?;
                }
                let mut copy = self.rclone();
                copy.extend([
                    "copyto".into(),
                    path(&marker),
                    format!("{remote}/.private-sync-access"),
                ]);
                self.run(&copy)?;
                args.extend(strings(&[
                    "--resync",
                    "--resync-mode",
                    if self.host == "fi" { "path1" } else { "path2" },
                ]));
                let mut size = self.rclone();
                size.extend(vec![
                    "size".into(),
                    remote.clone(),
                    "--json".into(),
                    "--drive-skip-gdocs".into(),
                    "--filter-from".into(),
                    path(&self.config.join("sync-filter.txt")),
                ]);
                let capacity: Value = serde_json::from_slice(&self.run(&size)?)?;
                let required = capacity["bytes"]
                    .as_u64()
                    .context("Missing remote size")?
                    .checked_add(5 << 30)
                    .context("Remote size overflow")?;
                ensure!(
                    free_space(&local)? >= required,
                    "Initial sync lacks space plus 5 GiB reserve"
                );
                let mut preview = args.clone();
                preview.push("--dry-run".into());
                self.run(&preview)?;
                let text = fs::read_to_string(&log)?.to_lowercase();
                ensure!(
                    !text.contains("duplicate object") && !text.contains("duplicate directory"),
                    "Duplicate Drive paths require review before sync"
                );
            }
            println!("↻ {folder}");
            if progress {
                args.extend(strings(&[
                    "--progress",
                    "--stats-one-line",
                    "--stats",
                    "1s",
                ]));
            }
            (if progress {
                self.runner.interactive(&mut command(&args))
            } else {
                self.run(&args).map(|_| ())
            })
            .with_context(|| format!("{folder}: sync failed; private log: {}", log.display()))?;
            atomic_json(
                &receipt,
                &json!({"completed":when,"host":self.host,"remote":remote}),
            )?;
            let text = fs::read_to_string(&log)?.to_lowercase();
            if text.contains("conflict") && (text.contains("renam") || text.contains("warning")) {
                self.notify(&format!(
                    "{folder}: conflict copies retained; review private sync logs"
                ));
            }
            self.prune_recovery(folder)?;
            println!("✓ {folder}");
        }
        atomic_json(
            &self.state.join("last-sync.json"),
            &json!({"completed":stamp(),"folders":folders}),
        )?;
        Ok(())
    }
    fn prune_recovery(&self, folder: &str) -> Result<()> {
        if !self.state.join("last-backup.json").exists() {
            return Ok(());
        }
        let cutoff = Utc::now() - chrono::Duration::days(self.p.recovery_days);
        let local = self.state.join("recovery").join(folder);
        no_symlinks(&local)?;
        if local.exists() {
            for e in fs::read_dir(local)? {
                let e = e?;
                if e.file_type()?.is_dir() && expired(&e.file_name().to_string_lossy(), cutoff) {
                    fs::remove_dir_all(e.path())?;
                }
            }
        }
        let remote = format!(
            "{}/backups/sync-recovery/{}/{folder}",
            self.p.remote, self.host
        );
        let mut args = self.rclone();
        args.extend(["lsf".into(), remote.clone(), "--dirs-only".into()]);
        let (code, bytes) = self.runner.guarded(&mut command(&args), None, true)?;
        if code == 0 {
            for name in String::from_utf8(bytes)?.lines() {
                let name = name.trim_end_matches('/');
                if expired(name, cutoff) {
                    let mut purge = self.rclone();
                    purge.extend(["purge".into(), format!("{remote}/{name}")]);
                    self.run(&purge)?;
                }
            }
        }
        Ok(())
    }
    fn backup(&self, scheduled: bool) -> Result<()> {
        // Validate sources before even initializing a remote repository.
        let paths: Vec<_> = self.p.folders.iter().map(|f| self.home.join(f)).collect();
        for p in &paths {
            no_symlinks(p)?;
            ensure!(
                p.is_dir(),
                "All private folders must be real directories before backup"
            );
        }
        let (common, ssh, root) = self.restic(&self.host)?;
        let mut coord = Coordinator::start(
            &ssh,
            &self.p.backup_root,
            &self.host,
            scheduled,
            self.runner,
        )?;
        let mut args = ssh.clone();
        args.push(repository_probe(&root));
        let (_, probe) =
            self.runner
                .guarded(&mut command(&args), Some(&mut coord.child.0), false)?;
        ensure!(
            [b"exists\n".as_slice(), b"missing\n".as_slice()].contains(&probe.as_slice()),
            "Unexpected repository probe response"
        );
        if probe == b"missing\n" {
            coord.run(&[common.clone(), strings(&["init"])].concat(), self.runner)?;
        }
        println!("⇧ Verifying incremental backup");
        let mut args = common.clone();
        args.extend(strings(&[
            "backup",
            "--host",
            &self.host,
            "--tag",
            "private-storage",
        ]));
        for pattern in [
            "**/.git",
            "**/node_modules",
            "**/.venv",
            "**/__pycache__",
            "**/.Trash-*",
            "**/.private-sync-access",
        ] {
            args.extend(strings(&["--exclude", pattern]));
        }
        args.extend(paths.iter().map(|p| path(p)));
        coord.run(&args, self.runner)?;
        coord.run(&[common.clone(), strings(&["check"])].concat(), self.runner)?;
        let bytes = coord.run(
            &[
                common.clone(),
                strings(&[
                    "snapshots",
                    "--json",
                    "--host",
                    &self.host,
                    "--tag",
                    "private-storage",
                ]),
            ]
            .concat(),
            self.runner,
        )?;
        let snapshots: Vec<Value> = serde_json::from_slice(&bytes)?;
        ensure!(
            !snapshots.is_empty()
                && snapshots.iter().all(|s| s["hostname"] == self.host
                    && s["tags"]
                        .as_array()
                        .is_some_and(|t| t.contains(&json!("private-storage")))),
            "Expected private-assets snapshots missing"
        );
        let keep = retention(&snapshots, Utc::now())?;
        let remove: Vec<_> = snapshots
            .iter()
            .filter_map(|s| s["id"].as_str())
            .filter(|id| !keep.contains(*id))
            .map(str::to_owned)
            .collect();
        if !remove.is_empty() {
            let mut forget = common;
            forget.push("forget".into());
            forget.extend(remove);
            forget.push("--prune".into());
            coord.run(&forget, self.runner)?;
        }
        coord.complete(self.runner)?;
        atomic_json(
            &self.state.join("last-backup.json"),
            &json!({"completed":stamp(),"repository":root}),
        )?;
        println!("✓ Incremental backup verified; retention applied");
        Ok(())
    }
    fn crypt_password(&self) -> Result<()> {
        let temp = tempfile::Builder::new()
            .prefix("crypt-password-")
            .tempdir_in(&self.state)?;
        let bytes = self.run(&[
            path(&self.home.join(".local/share/dotjik/home/.bws-wrapper.fish")),
            "secret".into(),
            "get".into(),
            self.p.crypt_secret_id.clone(),
        ])?;
        let secret: Value = serde_json::from_slice(&bytes)?;
        let plain = temp.path().join("config");
        fs::write(
            &plain,
            secret["value"]
                .as_str()
                .context("Bitwarden secret missing")?,
        )?;
        let encrypted = temp.path().join("config.age");
        self.runner.interactive(
            Command::new("age")
                .arg("-p")
                .arg("-o")
                .arg(&encrypted)
                .arg(&plain),
        )?;
        // Copy into a same-filesystem temporary before atomic replacement.
        let mut staged = tempfile::NamedTempFile::new_in(&self.config)?;
        std::io::copy(
            &mut crate::util::read_file(&encrypted)?,
            staged.as_file_mut(),
        )?;
        staged.as_file().sync_all()?;
        staged.persist(self.config.join("crypt.conf.age"))?;
        println!("✓ Crypt mount password set");
        Ok(())
    }
    fn mount(&self, which: &str) -> Result<()> {
        let subpath = match which {
            "crypt" => ".mnt/rclone/crypt",
            "giprivate" => ".mnt/google/private",
            "gipublic" => ".mnt/google/public",
            "siprivate" => ".mnt/si/private",
            _ => ".mnt/si/public",
        };
        let point = self.home.join(subpath);
        no_symlinks(&point)?;
        if mounted(&point) {
            println!("– Already mounted");
            return Ok(());
        }
        private_dir(&point)?;
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })))
            .join("private-storage");
        no_symlinks(&runtime)?;
        private_dir(&runtime)?;
        let (mut args, name) = if which == "crypt" {
            ensure!(
                self.config.join("crypt.conf.age").is_file(),
                "Set your crypt password first"
            );
            let temp = tempfile::tempdir_in(&runtime)?;
            let staged = temp.path().join("config");
            self.runner.interactive(
                Command::new("age")
                    .arg("-d")
                    .arg("-o")
                    .arg(&staged)
                    .arg(self.config.join("crypt.conf.age")),
            )?;
            let plain = runtime.join("crypt.conf");
            fs::rename(staged, &plain)?;
            (
                vec![
                    "rclone".into(),
                    "--config".into(),
                    path(&plain),
                    "mount".into(),
                    "crypt:".into(),
                    path(&point),
                ],
                "crypt".into(),
            )
        } else if let Some(kind) = which.strip_prefix("gi") {
            let mut a = self.rclone();
            a.extend([
                "mount".into(),
                format!("{}{kind}", self.p.drive_remote),
                path(&point),
                "--drive-skip-gdocs".into(),
            ]);
            (a, format!("gi-{kind}"))
        } else {
            let kind = &which[2..];
            (
                vec![
                    "rclone".into(),
                    "mount".into(),
                    format!(
                        ":sftp:{}/{kind}",
                        self.p.server_mount_root.trim_end_matches('/')
                    ),
                    path(&point),
                    "--sftp-ssh".into(),
                    shell_join(&self.ssh()?),
                    "--sftp-known-hosts-file".into(),
                    "none".into(),
                    "--sftp-shell-type".into(),
                    "unix".into(),
                    "--sftp-disable-hashcheck".into(),
                ],
                format!("si-{kind}"),
            )
        };
        args.extend([
            "--devname".into(),
            name,
            "--vfs-cache-mode".into(),
            "writes".into(),
        ]);
        let mut launch = strings(&[
            "systemd-run",
            "--user",
            "--collect",
            "--unit",
            &format!("private-mount-{which}"),
            "--property=Type=notify",
            "--property=Restart=on-failure",
            "--property=RestartSec=15",
            "--property=TimeoutStopSec=120",
        ]);
        launch.push(format!(
            "--property=ExecStopPost=-/usr/bin/fusermount3 -uz {}",
            quote(&path(&point))
        ));
        launch.extend(args);
        self.run(&launch)?;
        println!("↗ Mount started");
        Ok(())
    }
}
fn expired(name: &str, cutoff: DateTime<Utc>) -> bool {
    name.len() == 16
        && NaiveDateTime::parse_from_str(name, "%Y%m%dT%H%M%SZ").is_ok_and(|d| d.and_utc() < cutoff)
}
fn retention(snapshots: &[Value], now: DateTime<Utc>) -> Result<HashSet<String>> {
    let mut bands = BTreeMap::new();
    let mut years = BTreeMap::new();
    for s in snapshots {
        let id = s["id"].as_str().context("Snapshot ID missing")?;
        ensure!(
            !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid snapshot ID"
        );
        let when =
            DateTime::parse_from_rfc3339(s["time"].as_str().context("Snapshot time missing")?)?
                .with_timezone(&Utc);
        let age = (now - when).num_seconds().max(0) as f64 / 86400.;
        let slot = if age >= 365. {
            years.entry(when.year()).or_insert((when, id))
        } else {
            let band = [1., 3., 7., 14., 31., 92., 365.]
                .iter()
                .position(|end| age < *end)
                .unwrap();
            bands.entry(band).or_insert((when, id))
        };
        if when > slot.0 {
            *slot = (when, id);
        }
    }
    Ok(bands
        .values()
        .chain(years.values())
        .map(|(_, s)| s.to_string())
        .collect())
}
fn repository_probe(root: &str) -> String {
    // SSH executes through the account's login shell, which may be Fish.
    shell_join(&strings(&[
        "sh",
        "-c",
        "mkdir -p -- \"$1\" && if test -f \"$1/config\"; then echo exists; else echo missing; fi",
        "synctjik-repository-probe",
        root,
    ]))
}

// This transport only holds flock and saves an explicitly acknowledged receipt.
// All scheduling, receipt validation and retention decisions stay in Rust.
const COORDINATOR: &str = r#"
set -eu
umask 077
root=$1
host=$2
case "$host" in fi|li) ;; *) exit 2;; esac
state="$root/.coordination"
[ ! -L "$state" ] || exit 2
mkdir -p -- "$state"
[ ! -L "$state/backup.lock" ] || exit 2
exec 9>>"$state/backup.lock"
if ! flock -n 9; then printf 'SKIP another client backup is running\n'; exit 0; fi
printf 'READY\n'
if [ -e "$state/fi.json" ]; then
    [ -f "$state/fi.json" ] && [ ! -L "$state/fi.json" ] || exit 2
    printf 'RECEIPT '
    tr -d '\n' < "$state/fi.json"
    printf '\n'
else printf 'RECEIPT null\n'; fi
if IFS= read -r answer && [ "$answer" = complete ]; then
    staged=$(mktemp "$state/.receipt.XXXXXX")
    trap 'rm -f -- "$staged"' EXIT HUP INT TERM
    printf '{"host":"%s","completed_epoch":%s}\n' "$host" "$(date +%s)" > "$staged"
    mv -f -- "$staged" "$state/$host.json"
    printf 'SAVED\n'
fi
"#;
struct Coordinator {
    child: ManagedChild,
    replies: Receiver<String>,
}
impl Coordinator {
    fn start(
        ssh: &[String],
        root: &str,
        host: &str,
        scheduled: bool,
        runner: &Runner,
    ) -> Result<Self> {
        let remote = shell_join(&strings(&[
            "sh",
            "-c",
            COORDINATOR,
            "synctjik-coordinator",
            root,
            host,
        ]));
        let mut args = ssh.to_vec();
        args.push(remote);
        let mut s = Self::spawn(&mut command(&args))?;
        let reply = s.reply(runner)?;
        if let Some(reason) = reply.strip_prefix("SKIP ") {
            return Err(Busy(reason.into()).into());
        }
        ensure!(reply == "READY", "Unexpected backup coordinator response");
        let receipt = s.reply(runner)?;
        let value: Value = serde_json::from_str(
            receipt
                .strip_prefix("RECEIPT ")
                .context("Missing backup receipt response")?,
        )
        .context("Invalid backup receipt JSON")?;
        if host == "li" && scheduled && !value.is_null() {
            let midnight = Local::now()
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(Local)
                .earliest()
                .context("Ambiguous local midnight")?
                .timestamp() as f64;
            if peer_completed(&value, Utc::now().timestamp() as f64, midnight)? {
                return Err(Busy("fi already completed a verified backup today".into()).into());
            }
        }
        Ok(s)
    }
    fn spawn(command: &mut Command) -> Result<Self> {
        let mut child = ManagedChild(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()?,
            true,
        );
        let stdout = child.0.stdout.take().context("Missing coordination pipe")?;
        let (tx, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(s) => {
                        if tx.send(s).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self { child, replies })
    }
    fn reply(&mut self, runner: &Runner) -> Result<String> {
        let until = Instant::now() + Duration::from_secs(40);
        loop {
            runner.check()?;
            match self.replies.recv_timeout(Duration::from_millis(100)) {
                Ok(s) => return Ok(s),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("Lost backup coordination connection")
                }
                _ => (),
            }
            ensure!(
                Instant::now() < until,
                "Timed out waiting for backup coordination"
            );
        }
    }
    fn run(&mut self, args: &[String], runner: &Runner) -> Result<Vec<u8>> {
        runner
            .guarded(&mut command(args), Some(&mut self.child.0), false)
            .map(|(_, b)| b)
    }
    fn complete(&mut self, runner: &Runner) -> Result<()> {
        self.child
            .0
            .stdin
            .as_mut()
            .context("Coordination input closed")?
            .write_all(b"complete\n")?;
        ensure!(
            self.reply(runner)? == "SAVED",
            "Backup completion was not acknowledged"
        );
        Ok(())
    }
}
fn peer_completed(v: &Value, now: f64, midnight: f64) -> Result<bool> {
    let epoch = v["completed_epoch"]
        .as_f64()
        .context("Invalid desktop receipt timestamp")?;
    ensure!(
        v["host"] == "fi" && epoch.is_finite() && epoch >= 0. && epoch <= now + 300.,
        "Invalid desktop backup receipt"
    );
    Ok(epoch >= midnight)
}
pub fn main(args: &[String], runner: &Runner) -> Result<()> {
    let cli = match Cli::try_parse_from(
        std::iter::once("synctjik private-assets".into()).chain(args.iter().cloned()),
    ) {
        Ok(c) => c,
        Err(e) => {
            let code = e.exit_code();
            e.print()?;
            ensure!(code == 0, "Invalid private-assets arguments");
            return Ok(());
        }
    };
    let operation = match &cli.action {
        Action::Sync { .. } => "sync",
        Action::Init { .. } => "init",
        Action::Backup { .. } => "backup",
        Action::Snapshots { .. } => "snapshots",
        Action::Restore { .. } => "restore",
        Action::Status => "status",
        Action::Ready => "ready",
        Action::CryptPassword => "crypt-password",
        Action::Mount { .. } => "mount",
    };
    let app = App::load(runner)?;
    no_symlinks(&app.state)?;
    private_dir(&app.state)?;
    let result = (|| match cli.action {
        Action::Sync {
            folders,
            progress,
            max_delete_percent,
        } => {
            let _lock = Lock::take(&app.state.join("operation.lock"), false, runner)?;
            app.sync(false, &folders, progress, max_delete_percent)
        }
        Action::Init { folders } => {
            let _lock = Lock::take(&app.state.join("operation.lock"), false, runner)?;
            app.sync(true, &folders, false, 5)?;
            app.notify("Initial sync completed; hourly synchronization is ready");
            Ok(())
        }
        Action::Backup { scheduled } => {
            let _lock = Lock::take(&app.state.join("operation.lock"), true, runner)?;
            let _archive = Lock::take(
                &app.home.join(".local/state/device-archives/lock"),
                true,
                runner,
            )?;
            app.backup(scheduled)
        }
        Action::Ready => {
            ensure!(
                app.p.sharing_approved
                    && app.p.folders.iter().all(|f| app
                        .state
                        .join("bisync")
                        .join(f)
                        .join("initialized.json")
                        .exists()),
                "Private assets not initialized"
            );
            Ok(())
        }
        Action::Status => {
            for name in ["last-sync.json", "last-backup.json", "last-error.json"] {
                let p = app.state.join(name);
                println!(
                    "{name}: {}",
                    if p.exists() {
                        fs::read_to_string(p)?
                    } else {
                        "not completed".into()
                    }
                );
            }
            Ok(())
        }
        Action::Snapshots { source } => {
            let (mut cmd, _, _) = app.restic(source.as_deref().unwrap_or(&app.host))?;
            cmd.push("snapshots".into());
            print!("{}", String::from_utf8_lossy(&app.run(&cmd)?));
            Ok(())
        }
        Action::Restore {
            source,
            snapshot,
            to,
        } => {
            ensure!(
                fs::symlink_metadata(&to).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
                "Restore destination must be a new directory"
            );
            ensure!(
                snapshot == "latest"
                    || (!snapshot.is_empty() && snapshot.bytes().all(|b| b.is_ascii_hexdigit())),
                "Invalid restore snapshot"
            );
            let (mut cmd, _, _) = app.restic(&source)?;
            cmd.extend([
                "restore".into(),
                snapshot,
                "--target".into(),
                path(&to),
                "--verify".into(),
            ]);
            app.run(&cmd)?;
            Ok(())
        }
        Action::CryptPassword => app.crypt_password(),
        Action::Mount { which } => app.mount(&which),
    })();
    match result {
        Err(e) if e.is::<Busy>() => {
            println!("– {e}");
            Ok(())
        }
        Err(e) => {
            atomic_json(
                &app.state.join("last-error.json"),
                &json!({"time":stamp(),"operation":operation,"error":e.to_string()}),
            )?;
            app.notify("Private-assets operation failed; inspect private service status");
            Err(e)
        }
        Ok(()) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retention_keeps_newest_in_each_age_band_and_older_year() {
        let now = DateTime::parse_from_rfc3339("2026-10-29T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let ages = [
            0, 0, 1, 2, 3, 6, 7, 13, 14, 30, 31, 91, 92, 364, 365, 366, 730,
        ];
        let snapshots: Vec<_> = ages.iter().enumerate().map(|(i,days)|json!({"id":format!("{i:064x}"),"time":(now-chrono::Duration::days(*days)-chrono::Duration::seconds(i as i64)).to_rfc3339()})).collect();
        let keep = retention(&snapshots, now).unwrap();
        for i in [0, 2, 4, 6, 8, 10, 12, 14, 16] {
            assert!(keep.contains(&format!("{i:064x}")));
        }
        assert_eq!(keep.len(), 9);
        assert!(retention(&[json!({"id":"--bad","time":now.to_rfc3339()})], now).is_err());
        assert!(retention(&[json!({"id":"abc","time":"invalid"})], now).is_err());
    }
    #[test]
    fn receipts_fail_closed_and_recovery_uses_strict_names() {
        assert!(peer_completed(&json!({"host":"fi","completed_epoch":100}), 200., 50.).unwrap());
        assert!(!peer_completed(&json!({"host":"fi","completed_epoch":20}), 200., 50.).unwrap());
        for value in [
            json!({"host":"li","completed_epoch":100}),
            json!({"host":"fi","completed_epoch":501}),
            json!({"host":"fi","completed_epoch":"100"}),
            json!({}),
        ] {
            assert!(peer_completed(&value, 200., 50.).is_err());
        }
        assert!(expired("20240101T000000Z", Utc::now()));
        assert!(!expired("../20240101T000000Z", Utc::now()));
        assert!(!expired("20241301T000000Z", Utc::now()));
    }
    fn local_coordinator(root: &Path) -> Coordinator {
        Coordinator::spawn(
            Command::new("sh")
                .args(["-c", COORDINATOR, "test"])
                .arg(root)
                .arg("fi"),
        )
        .unwrap()
    }
    #[test]
    fn repository_probe_supports_posix_and_fish_login_shells() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("client's backup $(false)");
        for shell in ["sh", "fish"] {
            let result = Command::new(shell)
                .args(["-c", &repository_probe(root.to_str().unwrap())])
                .output()
                .unwrap();
            assert!(result.status.success(), "{shell}: {:?}", result.stderr);
            assert_eq!(result.stdout, b"missing\n");
        }
        fs::write(root.join("config"), b"fixture").unwrap();
        for shell in ["sh", "fish"] {
            let result = Command::new(shell)
                .args(["-c", &repository_probe(root.to_str().unwrap())])
                .output()
                .unwrap();
            assert!(result.status.success());
            assert_eq!(result.stdout, b"exists\n");
        }
    }
    #[test]
    fn coordinator_requires_explicit_success_and_excludes_other_clients() {
        let root = tempfile::tempdir().unwrap();
        let runner = Runner::new().unwrap();
        let mut first = local_coordinator(root.path());
        assert_eq!(first.reply(&runner).unwrap(), "READY");
        assert_eq!(first.reply(&runner).unwrap(), "RECEIPT null");
        let mut second = local_coordinator(root.path());
        assert!(second.reply(&runner).unwrap().starts_with("SKIP "));
        drop(second);
        first.child.0.stdin.take();
        first.child.0.wait().unwrap();
        drop(first);
        assert!(!root.path().join(".coordination/fi.json").exists());
        let mut completed = local_coordinator(root.path());
        completed.reply(&runner).unwrap();
        completed.reply(&runner).unwrap();
        completed.complete(&runner).unwrap();
        completed.child.0.wait().unwrap();
        let receipt = read_json(&root.path().join(".coordination/fi.json")).unwrap();
        assert_eq!(receipt["host"], "fi");
        assert!(receipt["completed_epoch"].as_i64().unwrap() > 0);
    }
    #[test]
    fn connection_loss_stops_transfer_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let runner = Runner::new().unwrap();
        let mut coord = Coordinator::spawn(Command::new("sh").args(["-c", "sleep 0.3"])).unwrap();
        let marker = dir.path().join("survived");
        let script = format!("(sleep 1; touch {}) & wait", quote(&path(&marker)));
        let started = Instant::now();
        assert!(coord
            .run(&strings(&["sh", "-c", &script]), &runner)
            .is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(1200));
        assert!(!marker.exists());
    }
}
