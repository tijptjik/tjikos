//! Shared durability and policy for native storage protection.
use super::process::*;
use crate::util::{atomic_json, read_json};
use anyhow::{ensure, Context, Result};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

pub fn policy<T: DeserializeOwned>(p: &Path) -> Result<T> {
    fn expand(v: &mut Value) -> Result<()> {
        match v {
            Value::String(s) => {
                if s.contains("${HOME}") {
                    *s = s.replace("${HOME}", &path(&home()?));
                }
                if s.contains("${XDG_RUNTIME_DIR}") {
                    let runtime = std::env::var("XDG_RUNTIME_DIR")
                        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
                    *s = s.replace("${XDG_RUNTIME_DIR}", &runtime);
                }
            }
            Value::Array(a) => {
                for v in a {
                    expand(v)?;
                }
            }
            Value::Object(a) => {
                for v in a.values_mut() {
                    expand(v)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    let mut v = read_json(p)?;
    expand(&mut v)?;
    serde_json::from_value(v).context("Invalid protection policy")
}
pub fn paths(paths: &[&Path]) -> Result<()> {
    for p in paths {
        no_symlinks(p)?;
    }
    Ok(())
}
pub fn text(r: &Runner, cmd: &mut Command) -> Result<String> {
    Ok(String::from_utf8(r.run(cmd)?)?.trim().into())
}
pub fn status(r: &Runner, cmd: &mut Command) -> Result<(i32, String)> {
    let (code, out) = r.guarded(cmd, None, true)?;
    Ok((code, String::from_utf8(out)?.trim().into()))
}
pub fn delay(r: &Runner, seconds: u64) -> Result<()> {
    let until = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < until {
        r.check()?;
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}
pub fn remove(p: &Path) -> Result<()> {
    if p.try_exists()? {
        fs::remove_file(p)?;
        fs::File::open(p.parent().context("Missing parent")?)?.sync_all()?;
    }
    Ok(())
}
pub fn empty(p: &Path) -> Result<()> {
    no_symlinks(p)?;
    ensure!(
        !p.exists() || (p.is_dir() && fs::read_dir(p)?.next().is_none()),
        "Destination must be empty"
    );
    Ok(())
}
pub fn new_target(p: &Path) -> Result<()> {
    no_symlinks(p)?;
    ensure!(!p.exists(), "Restore destination must not exist");
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(p)?;
    crate::util::private_dir(p)
}
pub fn exact_snapshot(s: &str) -> Result<()> {
    ensure!(
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Supply an exact 64-character snapshot ID"
    );
    Ok(())
}
pub fn host_guard(expected: &str) -> Result<()> {
    let actual = fs::read_to_string("/proc/sys/kernel/hostname")?;
    ensure!(
        actual.trim().split('.').next() == Some(expected),
        "This policy belongs to another host"
    );
    Ok(())
}
#[derive(Deserialize, Clone)]
pub struct Mounts {
    #[serde(default)]
    pub mounts: BTreeMap<PathBuf, String>,
    #[serde(default)]
    pub filesystems: BTreeMap<PathBuf, String>,
}
impl Mounts {
    pub fn check(&self, r: &Runner) -> Result<()> {
        for (mount, uuid) in &self.mounts {
            let actual = text(
                r,
                Command::new("findmnt")
                    .args(["-n", "-o", "UUID", "--mountpoint"])
                    .arg(mount),
            )?;
            ensure!(
                actual == *uuid,
                "Wrong or missing disk: {}",
                mount.display()
            );
            let options = text(
                r,
                Command::new("findmnt")
                    .args(["-n", "-o", "OPTIONS", "--mountpoint"])
                    .arg(mount),
            )?;
            ensure!(
                options.split(',').any(|o| o == "rw"),
                "Read-only disk: {}",
                mount.display()
            );
        }
        for (mount, expected) in &self.filesystems {
            let actual = text(
                r,
                Command::new("findmnt")
                    .args(["-n", "-o", "FSTYPE", "--mountpoint"])
                    .arg(mount),
            )?;
            ensure!(
                actual == *expected,
                "Unexpected filesystem at {}",
                mount.display()
            );
        }
        Ok(())
    }
}
#[derive(Deserialize, Clone)]
pub struct Restic {
    pub repository: PathBuf,
    pub password: PathBuf,
    pub cache: PathBuf,
    pub host: String,
    pub tag: String,
    pub keep_daily: u32,
    pub keep_weekly: u32,
    #[serde(default)]
    pub keep_monthly: u32,
}
impl Restic {
    pub fn validate(&self) -> Result<()> {
        paths(&[&self.repository, &self.password, &self.cache])?;
        ensure!(
            !self.host.is_empty()
                && !self.tag.is_empty()
                && self.keep_daily + self.keep_weekly + self.keep_monthly > 0,
            "Invalid Restic policy"
        );
        Ok(())
    }
    pub fn command(&self) -> Command {
        let mut c = Command::new("restic");
        c.args(["--retry-lock", "15m"])
            .env("RESTIC_REPOSITORY", &self.repository)
            .env("RESTIC_PASSWORD_FILE", &self.password)
            .env("RESTIC_CACHE_DIR", &self.cache);
        c
    }
    pub fn init(&self, r: &Runner, args: &[String]) -> Result<()> {
        self.validate()?;
        secret(&self.password)?;
        if !self.repository.join("config").is_file() {
            empty(&self.repository)?;
            r.run(self.command().arg("init").args(args))?;
        }
        Ok(())
    }
    pub fn snapshots(&self, r: &Runner) -> Result<Vec<Value>> {
        Ok(serde_json::from_slice(&r.run(self.command().args([
            "snapshots",
            "--host",
            &self.host,
            "--tag",
            &self.tag,
            "--json",
        ]))?)?)
    }
    pub fn retain(&self, r: &Runner, tag: &str) -> Result<()> {
        r.run(self.command().args([
            "forget",
            "--host",
            &self.host,
            "--tag",
            tag,
            "--group-by",
            "host,tags",
            "--keep-daily",
            &self.keep_daily.to_string(),
            "--keep-weekly",
            &self.keep_weekly.to_string(),
            "--keep-monthly",
            &self.keep_monthly.to_string(),
            "--prune",
        ]))?;
        Ok(())
    }
}
pub fn record_error(state: &Path, result: &Result<()>) -> Result<()> {
    if let Err(e) = result {
        atomic_json(
            &state.join("last-error.json"),
            &serde_json::json!({"time":crate::util::now(), "error":e.to_string()}),
        )?;
    }
    Ok(())
}
pub fn walk(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for item in fs::read_dir(root)? {
        let item = item?;
        let t = item.file_type()?;
        if t.is_dir() {
            out.extend(walk(&item.path())?);
        } else if t.is_file() {
            out.push(item.path());
        }
    }
    Ok(out)
}
