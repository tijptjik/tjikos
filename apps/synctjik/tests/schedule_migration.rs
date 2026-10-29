use std::{fs, os::unix::fs::PermissionsExt, process::Command};
#[test]
fn running_backup_prevents_timer_changes() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let script = bin.join("systemctl");
    fs::write(&script,"#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$CALLS\"\nif [ \"$2\" = show ]; then printf 'LoadState=loaded\\nActiveState=active\\n'; else exit 90; fi\n").unwrap();
    fs::set_permissions(script, fs::Permissions::from_mode(0o755)).unwrap();
    let calls = dir.path().join("calls");
    let output = Command::new(env!("CARGO_BIN_EXE_synctjik"))
        .args(["migrate-schedules", "--retire"])
        .env("HOME", dir.path())
        .env("PATH", bin)
        .env("CALLS", &calls)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("backup running"));
    assert!(!fs::read_to_string(calls).unwrap().contains("disable"));
}
#[test]
fn no_old_units_is_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_synctjik"))
        .arg("migrate-schedules")
        .env("HOME", dir.path())
        .env("PATH", dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("current"));
}
