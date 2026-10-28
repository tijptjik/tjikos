use crate::util::*;
use anyhow::{ensure, Result};
use std::{fs, io, os::unix::fs::OpenOptionsExt, path::Path, process::Command};
pub struct Store {
    pub root: String,
}
impl Store {
    pub fn local(&self) -> bool {
        !self.root.contains(':')
    }
    fn path(&self, key: &str) -> Result<String> {
        safe_name(key)?;
        Ok(format!("{}/{}", self.root.trim_end_matches('/'), key))
    }
    pub fn get(&self, key: &str, dest: &Path) -> Result<()> {
        let path = self.path(key)?;
        if self.local() {
            let mut f = read_file(Path::new(&path))?;
            let mut out = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(dest)?;
            io::copy(&mut f, &mut out)?;
        } else {
            run(Command::new("rclone").args(["copyto", &path]).arg(dest))?;
        }
        Ok(())
    }
    pub fn put(&self, path: &Path, key: &str, immutable: bool) -> Result<()> {
        let target = self.path(key)?;
        if self.local() {
            let dest = Path::new(&target);
            private_dir(dest.parent().unwrap())?;
            if immutable {
                let mut out = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(dest)?;
                io::copy(&mut read_file(path)?, &mut out)?;
                out.sync_all()?;
            } else {
                let mut out = tempfile::NamedTempFile::new_in(dest.parent().unwrap())?;
                io::copy(&mut read_file(path)?, &mut out)?;
                out.as_file().sync_all()?;
                out.persist(dest)?;
            }
        } else {
            let mut cmd = Command::new("rclone");
            cmd.arg("copyto").arg(path).arg(target);
            if immutable {
                cmd.arg("--immutable");
            }
            run(&mut cmd)?;
        }
        Ok(())
    }
    pub fn list(&self, prefix: &str, completed: bool) -> Result<Vec<String>> {
        let target = self.path(prefix)?;
        let mut names = Vec::new();
        if self.local() {
            if Path::new(&target).exists() {
                for e in fs::read_dir(target)? {
                    let e = e?;
                    if !completed || e.path().join("complete.json").is_file() {
                        names.push(e.file_name().to_string_lossy().into_owned());
                    }
                }
            }
        } else {
            let mut cmd = Command::new("rclone");
            cmd.args(["lsf", &target]);
            if completed {
                cmd.args(["--recursive", "--files-only", "--include", "complete.json"]);
            } else {
                cmd.args(["--max-depth", "1"]);
            }
            let out = cmd.output()?;
            ensure!(out.status.success(), "Remote listing failed");
            for l in String::from_utf8(out.stdout)?.lines() {
                if completed {
                    if let Some(id) = l.strip_suffix("/complete.json") {
                        if !id.contains('/') {
                            names.push(id.into())
                        }
                    }
                } else {
                    names.push(l.trim_end_matches('/').into());
                }
            }
        }
        names.sort();
        Ok(names)
    }
    pub fn remove(&self, prefix: &str, snap: &str) -> Result<()> {
        snapshot(snap)?;
        let target = self.path(&format!("{prefix}/{snap}"))?;
        if self.local() {
            fs::remove_dir_all(target)?;
        } else {
            run(Command::new("rclone").args(["purge", &target]))?;
        }
        Ok(())
    }
}
