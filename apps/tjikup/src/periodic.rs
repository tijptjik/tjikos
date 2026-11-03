//! Success-only cadence for declarative maintenance recipes.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    completed_at: u64,
}

pub struct Periodic {
    path: PathBuf,
    interval: u64,
    _lock: crate::process::Lock,
}

fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
fn date(seconds: u64) -> Result<String> {
    Ok(chrono::DateTime::from_timestamp(seconds.try_into()?, 0)
        .context("invalid periodic receipt time")?
        .format("%Y-%m-%d %H:%M UTC")
        .to_string())
}

impl Periodic {
    pub fn open(id: &str, interval: u64) -> Result<Self> {
        let directory = crate::scheduler::state_root().join("periodic");
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&directory)?;
        if !fs::symlink_metadata(&directory)?.file_type().is_dir() {
            bail!("periodic state must be a real directory");
        }
        // Recipe IDs have already been validated by Config. This lock also
        // serializes the same recipe invoked through two different configs.
        let lock = crate::process::Lock::acquire(&directory.join(format!("{id}.lock")))?;
        Ok(Self {
            path: directory.join(format!("{id}.json")),
            interval,
            _lock: lock,
        })
    }

    pub fn next_due(&self) -> Result<Option<String>> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&self.path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let receipt: Receipt =
            serde_json::from_reader(file).context("invalid periodic success receipt")?;
        let current = now()?;
        if receipt.version != 1 || receipt.completed_at > current {
            bail!("invalid periodic receipt version or future completion time");
        }
        let next = receipt
            .completed_at
            .checked_add(self.interval)
            .context("periodic time overflow")?;
        Ok(if current < next {
            Some(date(next)?)
        } else {
            None
        })
    }

    pub fn complete(&self) -> Result<String> {
        let completed_at = now()?;
        let directory = self.path.parent().context("missing receipt directory")?;
        let mut staged = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer(
            &mut staged,
            &Receipt {
                version: 1,
                completed_at,
            },
        )?;
        staged.write_all(b"\n")?;
        staged.as_file().sync_all()?;
        staged.persist(&self.path)?;
        fs::File::open(directory)?.sync_all()?;
        date(completed_at + self.interval)
    }
}
