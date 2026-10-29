//! Hold the legacy locks across a caller's reviewed configuration cutover.
use super::{process::*, protection::*};
use anyhow::{ensure, Result};
use clap::Parser;
use serde::Deserialize;
use std::{path::PathBuf, process::Command};
#[derive(Parser)]
#[command(
    name = "synctjik protection-migration",
    about = "Defer protection routing changes while jobs are active; optionally run a reviewed apply while holding their locks"
)]
struct Cli {
    #[arg(long)]
    policy: PathBuf,
    #[arg(last = true)]
    command: Vec<String>,
}
#[derive(Deserialize)]
struct Unit {
    name: String,
    #[serde(default)]
    user: bool,
}
#[derive(Deserialize)]
struct Policy {
    locks: Vec<PathBuf>,
    units: Vec<Unit>,
    #[serde(default)]
    reload_managers: Vec<String>,
}
pub fn main(args: &[String], r: &Runner) -> Result<()> {
    let cli = Cli::parse_from(
        std::iter::once("synctjik protection-migration".into()).chain(args.iter().cloned()),
    );
    let p: Policy = policy(&cli.policy)?;
    ensure!(
        p.reload_managers
            .iter()
            .all(|scope| ["user", "system"].contains(&scope.as_str())),
        "Invalid systemd manager scope"
    );
    let mut locks = Vec::new();
    for path in &p.locks {
        // Keep /var/run and /run/lock compatibility without changing shared modes.
        absolute(path)?;
        locks.push(Lock::take(path, false, r)?);
    }
    for unit in p.units {
        crate::util::host(&unit.name)?;
        let mut c = Command::new("systemctl");
        if unit.user {
            c.arg("--user");
        }
        let output = text(
            r,
            c.args(["show", &unit.name, "--property=LoadState,ActiveState"]),
        )?;
        let state = output.lines().find_map(|l| l.strip_prefix("ActiveState="));
        ensure!(state.is_some(), "Cannot inspect protection worker state");
        if state.is_some_and(|s| ["active", "activating", "deactivating", "reloading"].contains(&s))
        {
            // RemainAfterExit resume services are intentionally excluded by policy.
            return Err(
                Busy("Protection worker active; defer migration until it finishes".into()).into(),
            );
        }
    }
    if cli.command.is_empty() {
        println!("Protection migration ready; no schedules changed");
    } else {
        r.supervised(&mut command(&cli.command))?;
        // Load new ExecStart/ExecStopPost routing before releasing workers.
        // daemon-reload never starts or restarts services or enables timers.
        for scope in &p.reload_managers {
            let mut reload = Command::new("systemctl");
            if scope == "user" {
                reload.arg("--user");
            }
            r.run(reload.arg("daemon-reload"))?;
        }
    }
    drop(locks);
    Ok(())
}
