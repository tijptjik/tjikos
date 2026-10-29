//! Retire timers without stopping their running archive jobs.
use anyhow::{bail, Result};
use std::{collections::BTreeMap, path::PathBuf, process::Command};
const OLD: &[&str] = &[
    "backup-packages",
    "synctjik-device-collections-backup",
    "cloudflare-restic-backup",
    "private-backup",
    "private-sync",
];
fn state(unit: &str) -> Result<BTreeMap<String, String>> {
    let output = Command::new("systemctl")
        .args(["--user", "show", unit, "--property=LoadState,ActiveState"])
        .output()?;
    if !output.status.success() {
        bail!("cannot inspect schedule state");
    }
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.into(), v.into())))
        .collect())
}
pub fn run(retire: bool) -> Result<()> {
    let root =
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config/systemd/user");
    let names: Vec<_> = OLD
        .iter()
        .copied()
        .filter(|name| {
            retire
                || [".service", ".timer"]
                    .iter()
                    .any(|ext| root.join(format!("{name}{ext}")).symlink_metadata().is_ok())
        })
        .collect();
    let busy = |names: &[&str]| -> Result<bool> {
        for name in names {
            if state(&format!("{name}.service"))?
                .get("ActiveState")
                .is_some_and(|s| {
                    ["active", "activating", "deactivating", "reloading"].contains(&s.as_str())
                })
            {
                return Ok(true);
            }
        }
        Ok(false)
    };
    if busy(&names)? {
        bail!("backup running; retry schedule migration after it finishes");
    }
    if !retire {
        println!(
            "SKIP    ✓ Synctjik (schedule migration)  {}",
            if names.is_empty() { "current" } else { "ready" }
        );
        return Ok(());
    }
    let mut changed = false;
    for name in &names {
        let timer = format!("{name}.timer");
        if state(&timer)?
            .get("LoadState")
            .is_some_and(|s| s == "loaded")
        {
            if !Command::new("systemctl")
                .args(["--user", "disable", "--now", &timer])
                .output()?
                .status
                .success()
            {
                bail!("failed to retire old timer");
            }
            changed = true;
        }
    }
    if busy(&names)? {
        bail!("backup running; retry schedule migration after it finishes");
    }
    println!(
        "{}    ✓ Synctjik (schedule migration)  {}",
        if changed { "CONFIG" } else { "SKIP" },
        if changed {
            "legacy timers retired"
        } else {
            "current"
        }
    );
    Ok(())
}
