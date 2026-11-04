//! Read-only status of Dotjik's systemd-owned screen recorder.
use anyhow::{Context, Result};
use gio::glib::variant::ToVariant;
use serde_json::json;
use std::{collections::HashSet, path::Path, time::Duration};

fn recording(proc_root: &Path, main_pid: u32) -> bool {
    let mut pending = vec![main_pid];
    let mut visited = HashSet::new();
    while let Some(pid) = pending.pop() {
        if pid == 0 || !visited.insert(pid) || visited.len() > 64 {
            continue;
        }
        let process = proc_root.join(pid.to_string());
        if std::fs::read_link(process.join("exe"))
            .ok()
            .and_then(|p| p.file_name().map(|s| s == "gpu-screen-recorder"))
            == Some(true)
        {
            return true;
        }
        // The service's Python main thread owns selection/countdown/recorder children.
        if let Ok(children) = std::fs::read_to_string(process.join(format!("task/{pid}/children")))
        {
            pending.extend(
                children
                    .split_whitespace()
                    .filter_map(|s| s.parse::<u32>().ok()),
            );
        }
    }
    false
}

fn main_pid(connection: &gio::DBusConnection) -> Result<u32> {
    let reply = connection.call_sync(
        Some("org.freedesktop.systemd1"),
        "/org/freedesktop/systemd1/unit/screen_2drecord_2eservice",
        "org.freedesktop.DBus.Properties",
        "Get",
        Some(&("org.freedesktop.systemd1.Service", "MainPID").to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        250,
        gio::Cancellable::NONE,
    )?;
    reply
        .child_value(0)
        .as_variant()
        .and_then(|v| v.get::<u32>())
        .context("Invalid screen recorder MainPID")
}

pub async fn watch() -> Result<()> {
    let connection = tokio::task::spawn_blocking(|| {
        gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
    })
    .await??;
    let mut previous = None;
    let mut ticker = tokio::time::interval(Duration::from_millis(250));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        anyhow::ensure!(!connection.is_closed(), "Session bus disconnected");
        let connection = connection.clone();
        // Only finite queries run on the blocking pool so SIGTERM can stop the helper.
        let current = tokio::task::spawn_blocking(move || {
            let pid = main_pid(&connection);
            (pid.is_ok(), recording(Path::new("/proc"), pid.unwrap_or(0)))
        })
        .await?;
        if previous != Some(current) {
            crate::emit(&json!({"available":current.0,"recording":current.1}));
            previous = Some(current);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};

    #[test]
    fn only_recorder_in_service_tree_counts() {
        let root = tempfile::tempdir().unwrap();
        for (pid, executable, children) in [
            (10, "/usr/bin/python3", "11"),
            (11, "/usr/bin/slurp", ""),
            (20, "/usr/bin/gpu-screen-recorder", ""),
        ] {
            let p = root.path().join(pid.to_string());
            fs::create_dir_all(p.join(format!("task/{pid}"))).unwrap();
            symlink(executable, p.join("exe")).unwrap();
            fs::write(p.join(format!("task/{pid}/children")), children).unwrap();
        }
        assert!(!recording(root.path(), 0));
        assert!(!recording(root.path(), 10)); // unrelated recorder and selection do not count
        fs::remove_file(root.path().join("11/exe")).unwrap();
        symlink("/usr/bin/python3", root.path().join("11/exe")).unwrap();
        assert!(!recording(root.path(), 10)); // countdown does not count
        fs::write(root.path().join("10/task/10/children"), "20").unwrap();
        assert!(recording(root.path(), 10));
        fs::remove_file(root.path().join("20/exe")).unwrap();
        assert!(!recording(root.path(), 10)); // stopped or crashed child clears the dot
        fs::write(root.path().join("10/task/10/children"), "10 invalid 999").unwrap();
        assert!(!recording(root.path(), 10));
    }
}
