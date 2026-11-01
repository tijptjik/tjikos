use anyhow::{Context, Result};
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

// File locking is shared by all GUI instances and the headless service. It spans
// the complete scan and receipt writes, including network delivery.
pub fn lock(dir: &Path) -> Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(dir.join("sync.lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(error) => Err(error.into()),
    }
}
pub fn run(dir: std::path::PathBuf, once: bool) -> Result<()> {
    let _lock = lock(&dir)?.context("Email sync is already running for this account")?;
    let (commands, events) = crate::worker::start(dir.clone(), true);
    if !once {
        crate::push::start(dir, commands.clone());
    }
    for event in events {
        match event {
            crate::worker::Event::SyncComplete => {
                println!("Email sync completed");
                if once {
                    return Ok(());
                }
            }
            crate::worker::Event::SyncError(_) | crate::worker::Event::Error(_) => {
                // Do not journal email content, OAuth responses, addresses or tokens.
                eprintln!(
                    "Email sync failed; check connectivity and account authorization in Gmailtjik"
                );
                if once {
                    anyhow::bail!("Email sync failed")
                }
            }
            _ => {}
        }
    }
    anyhow::bail!("Email sync worker stopped")
}
#[cfg(test)]
mod tests {
    #[test]
    fn ownership_is_exclusive_and_released_on_exit() {
        let dir = tempfile::tempdir().unwrap();
        let owner = super::lock(dir.path()).unwrap().unwrap();
        assert!(super::lock(dir.path()).unwrap().is_none());
        drop(owner);
        assert!(super::lock(dir.path()).unwrap().is_some());
    }
}
