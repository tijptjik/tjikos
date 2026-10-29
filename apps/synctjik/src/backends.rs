//! Embedded, tested backup engines. Device policy and credentials stay outside the tool.
use anyhow::{Context, Result};
use std::{os::unix::process::CommandExt, process::Command};

pub const CLOUDFLARE: &str = include_str!("../runtime/cloudflare_backup.py");
pub const PRIVATE_ASSETS: &str = include_str!("../runtime/private_assets.py");

pub fn exec(source: &str, args: &[String]) -> Result<()> {
    // Replace this process: systemd keeps its PID, signal handling and job lifetime.
    // The engine bytes are pinned in the executable; an update cannot replace
    // Python modules underneath an already-running backup worker.
    let error = Command::new("python3")
        .arg("-c")
        .arg(source)
        .args(args)
        .exec();
    Err(error).context("Unable to start Synctjik's Python backup engine")
}
