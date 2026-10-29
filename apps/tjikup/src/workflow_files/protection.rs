//! Shared interface: Synctjik owns worker/lock checks; Tjikup owns file deployment.
use anyhow::{Context, Result};
use std::{path::Path, process::Command};

/// Wrap only an explicitly declared protection cutover. No policy is duplicated
/// here, and a missing/incompatible Synctjik must fail closed before file writes.
pub(super) fn apply(
    repo: &Path,
    host: &str,
    scope: &str,
    group: Option<&str>,
    native: &[String],
) -> Result<Option<i32>> {
    let policy_scope = match scope {
        "system" => "system",
        "home-files" if group.is_none_or(|g| g == "base") => "home",
        _ => return Ok(None),
    };
    let policy = repo.join(format!(
        "native/protection-migration/{policy_scope}-{host}.json"
    ));
    if !policy.is_file() {
        return Ok(None);
    }
    let mut command = if scope == "system" {
        let mut command = Command::new("sudo");
        command.args(["-n", "--", "/usr/local/bin/synctjik"]);
        command
    } else {
        Command::new(super::home()?.join(".local/bin/synctjik"))
    };
    command
        .arg("protection-migration")
        .arg("--policy")
        .arg(policy)
        .arg("--");
    if scope == "system" {
        // workflow_files already staged and verified this root-owned runtime.
        command.arg("/usr/local/lib/tjikup/tjikup");
    } else {
        command.arg(std::env::current_exe()?);
    }
    let status = command.arg("files").args(native).status()
        .context("Protection migration unavailable; install a compatible Synctjik before applying protection files")?;
    Ok(Some(status.code().unwrap_or(1)))
}
