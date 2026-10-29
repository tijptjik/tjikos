use serde_json::{json, Value};
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tjikup"))
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("tjikup")).unwrap();
    dir
}
#[test]
fn context_reuses_enrollment_aliases_and_dependencies() {
    let dir = fixture();
    let host = String::from_utf8(Command::new("hostname").output().unwrap().stdout)
        .unwrap()
        .trim()
        .to_owned();
    let profile = dir.path().join("device.json");
    fs::write(
        &profile,
        serde_json::to_vec(
            &json!({"version":1,"hostname":host,"role":"desktop","subscriptions":["old"]}),
        )
        .unwrap(),
    )
    .unwrap();
    fs::write(dir.path().join("tjikup.json"),serde_json::to_vec(&json!({"version":2,"device_file":profile,"subscription_aliases":{"old":"new"},"recipes":[{"id":"base","apply":["true"]},{"id":"new","optional":true,"needs":["base"],"apply":["true"]},{"id":"other","optional":true,"apply":["true"]}]})).unwrap()).unwrap();
    let output = binary()
        .args(["workflow", "--repo"])
        .arg(dir.path())
        .arg("context")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let context: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(context["subscriptions"], json!(["base", "new"]));
    assert_eq!(context["role"], "desktop");
}
#[test]
fn capture_preflights_every_rule_before_writing() {
    let dir = fixture();
    let home = dir.path().join("home");
    fs::create_dir(&home).unwrap();
    fs::write(dir.path().join("first"), "original").unwrap();
    fs::write(home.join("first"), "changed").unwrap();
    fs::write(dir.path().join("second"), r#"{"nested":{"x":1}}"#).unwrap();
    fs::write(home.join("second"), r#"{"nested":false}"#).unwrap();
    fs::write(
        dir.path().join("tjikup/capture.json"),
        serde_json::to_vec(&json!([
            {"name":"first","source":"first","target":"first","kind":"copy"},
            {"name":"second","source":"second","target":"second","kind":"jsonc-scalars"}
        ]))
        .unwrap(),
    )
    .unwrap();
    let output = binary()
        .args(["workflow", "--repo"])
        .arg(dir.path())
        .arg("capture")
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(dir.path().join("first")).unwrap(),
        "original"
    );
}
#[test]
fn service_refresh_never_restarts_active_oneshot() {
    let dir = fixture();
    fs::create_dir_all(dir.path().join("scripts")).unwrap();
    fs::create_dir_all(dir.path().join("native/home/files/.config/systemd/user")).unwrap();
    fs::create_dir(dir.path().join("bin")).unwrap();
    fs::write(
        dir.path().join("scripts/user-service-inputs.json"),
        r#"{"archive.service":[]}"#,
    )
    .unwrap();
    let mock = dir.path().join("bin/systemctl");
    fs::write(&mock,"#!/bin/sh\ncase \"$2\" in show) printf 'LoadState=loaded\\nActiveState=active\\nType=oneshot\\nNeedDaemonReload=no\\n';; *) exit 90;; esac\n").unwrap();
    fs::set_permissions(mock, fs::Permissions::from_mode(0o755)).unwrap();
    let output = binary()
        .args(["workflow", "--repo"])
        .arg(dir.path())
        .arg("services")
        .env("PATH", dir.path().join("bin"))
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("refusing to restart job"));
}
