//! Locker service adapter; UI code never launches session commands.
//! Hypridle's configured lock command remains the host policy authority even
//! while idle inhibition has stopped its daemon.
use anyhow::{ensure, Context, Result};
use std::{path::Path, process::Stdio, time::Duration};

pub async fn lock(policy: &Path) -> Result<()> {
    let text = std::fs::read_to_string(policy).context("Could not read locker policy")?;
    let command = text
        .lines()
        .find_map(|line| {
            let (key, value) = line.trim().split_once('=')?;
            (key.trim() == "lock_cmd").then(|| value.trim())
        })
        .filter(|s| !s.is_empty())
        .context("No lock_cmd configured in idle policy")?;
    // This is the user's shell command from trusted local desktop configuration,
    // not text supplied by a panel or a provider payload.
    let mut child = tokio::process::Command::new("sh")
        .args(["-c", command])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Could not launch configured locker")?;
    match tokio::time::timeout(Duration::from_millis(300), child.wait()).await {
        Ok(status) => ensure!(status?.success(), "Configured locker failed to start"),
        Err(_) => {
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
        }
    }
    // Startup acknowledgement only; never claim that the session is locked.
    Ok(())
}
