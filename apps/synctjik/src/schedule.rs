use crate::{engine::Engine, util::*};
use anyhow::{ensure, Result};
use std::{fs, io::Write, path::Path, process::Command};
/// Dotfiles LFS units use the same domain/target/action names as fleet policy.
pub fn units(role: &str) -> Result<(String, String)> {
    let minute = match role {
        "desktop" => "00",
        "laptop" => "15",
        "server" => "30",
        _ => anyhow::bail!("Unknown device role"),
    };
    Ok(("[Unit]\nDescription=Verified Dotfiles LFS backups (synctjik)\n\n[Service]\nType=oneshot\nTimeoutStartSec=infinity\nTimeoutStopSec=30\nUMask=0077\nExecCondition=%h/.local/bin/synctjik ready\nExecStart=%h/.local/bin/synctjik backup\n".into(),format!("[Unit]\nDescription=Daily Dotfiles LFS backup\n\n[Timer]\nOnCalendar=*-*-* 00:{minute}:00\nAccuracySec=1s\nRandomizedDelaySec=0\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n")))
}
pub fn install(app: &Engine, role: &str, config_dir: &Path) -> Result<()> {
    app.ready()?;
    ensure!(
        app.home.join(".local/bin/synctjik").is_file(),
        "Install synctjik before enabling its schedule"
    );
    let legacy = ["backup-packages", "synctjik-device-collections-backup"];
    let mut timers = Vec::new();
    for name in legacy {
        legacy_idle(name)?;
        let timer = format!("{name}.timer");
        let loaded = Command::new("systemctl")
            .args(["--user", "show", &timer, "--property=LoadState", "--value"])
            .output()?;
        ensure!(
            loaded.status.success(),
            "Cannot inspect legacy archive timer"
        );
        if String::from_utf8_lossy(&loaded.stdout).trim() == "loaded" {
            timers.push(timer);
        }
    }
    let (service, timer) = units(role)?;
    let dir = config_dir.join("systemd/user");
    fs::create_dir_all(&dir)?;
    for (name, data) in [
        ("synctjik-dotfiles-lfs-backup.service", service),
        ("synctjik-dotfiles-lfs-backup.timer", timer),
    ] {
        let path = dir.join(name);
        ensure!(!path.is_symlink(), "Refusing symlinked schedule unit");
        let mut tmp = tempfile::NamedTempFile::new_in(&dir)?;
        tmp.write_all(data.as_bytes())?;
        tmp.as_file().sync_all()?;
        tmp.persist(path)?;
    }
    for timer in timers {
        run(Command::new("systemctl").args(["--user", "disable", "--now", &timer]))?;
    }
    // A timer could have triggered after the initial check; never overlap jobs.
    for name in legacy {
        legacy_idle(name)?;
    }
    run(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
    run(Command::new("systemctl").args([
        "--user",
        "enable",
        "--now",
        "synctjik-dotfiles-lfs-backup.timer",
    ]))?;
    Ok(())
}
fn legacy_idle(name: &str) -> Result<()> {
    let unit = format!("{name}.service");
    let state = Command::new("systemctl")
        .args(["--user", "show", &unit, "--property=ActiveState", "--value"])
        .output()?;
    ensure!(
        state.status.success(),
        "Cannot inspect legacy archive service"
    );
    ensure!(
        matches!(
            String::from_utf8_lossy(&state.stdout).trim(),
            "inactive" | "failed" | ""
        ),
        "Legacy archive job is running; retry schedule migration after it finishes"
    );
    Ok(())
}
