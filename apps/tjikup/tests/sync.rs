use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn sync_forwards_literal_arguments_and_exit_status_without_loading_config() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("synctjik");
    fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' \"$@\"\nexit 23\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tjikup"))
        .args(["sync", "plan", "--source", "host with spaces", "$(false)"])
        .env("PATH", dir.path())
        .env("TJIKUP_CONFIG", dir.path().join("nonexistent"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(23));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "plan\n--source\nhost with spaces\n$(false)\n"
    );
}

#[test]
fn help_advertises_sync() {
    let output = Command::new(env!("CARGO_BIN_EXE_tjikup"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("sync <command>"));
    assert!(!text.contains("arc <command>"));
}
