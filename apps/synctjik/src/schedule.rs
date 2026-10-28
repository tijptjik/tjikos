use crate::{engine::Engine, util::*};
use anyhow::{ensure, Result};
use std::{fs, io::Write, path::Path, process::Command};
/// Keep the historical unit name so upgrades do not create a second schedule.
pub fn units(role: &str) -> Result<(String, String)> {
    let minute = match role {
        "desktop" => "00",
        "laptop" => "15",
        "server" => "30",
        _ => anyhow::bail!("Unknown device role"),
    };
    Ok(("[Unit]\nDescription=Verified device archives (synctjik)\n\n[Service]\nType=oneshot\nTimeoutStartSec=infinity\nTimeoutStopSec=30\nUMask=0077\nExecCondition=%h/.local/bin/synctjik ready\nExecStart=%h/.local/bin/synctjik backup\n".into(),format!("[Unit]\nDescription=Daily device archives\n\n[Timer]\nOnCalendar=*-*-* 00:{minute}:00\nAccuracySec=1s\nRandomizedDelaySec=0\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n")))
}
pub fn install(app: &Engine, role: &str, config_dir: &Path) -> Result<()> {
    app.ready()?;
    ensure!(
        app.home.join(".local/bin/synctjik").is_file(),
        "Install synctjik before enabling its schedule"
    );
    let (service, timer) = units(role)?;
    let dir = config_dir.join("systemd/user");
    fs::create_dir_all(&dir)?;
    for (name, data) in [
        ("backup-packages.service", service),
        ("backup-packages.timer", timer),
    ] {
        let path = dir.join(name);
        if path.is_symlink() {
            anyhow::bail!("Refusing symlinked schedule unit");
        }
        let mut tmp = tempfile::NamedTempFile::new_in(&dir)?;
        tmp.write_all(data.as_bytes())?;
        tmp.as_file().sync_all()?;
        tmp.persist(path)?;
    }
    run(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
    run(Command::new("systemctl").args(["--user", "enable", "--now", "backup-packages.timer"]))?;
    Ok(())
}
