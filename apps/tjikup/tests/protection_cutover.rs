use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

#[test]
fn standalone_file_apply_defers_before_writes_and_delegates_when_idle() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let policy = root.join("native/home");
    fs::create_dir_all(home.join(".local/bin")).unwrap();
    fs::create_dir_all(&policy).unwrap();
    fs::create_dir_all(root.join("native/protection-migration")).unwrap();
    let host = fs::read_to_string("/proc/sys/kernel/hostname")
        .unwrap()
        .trim()
        .to_owned();
    fs::write(
        root.join("device.json"),
        json!({"version":1,"hostname":host,"role":"laptop","subscriptions":[]}).to_string(),
    )
    .unwrap();
    fs::write(root.join("tjikup.json"),json!({"version":2,"device_file":root.join("device.json"),"recipes":[{"id":"base","apply":["true"]}]}).to_string()).unwrap();
    fs::write(
        root.join(format!("native/protection-migration/home-{host}.json")),
        "{}",
    )
    .unwrap();
    fs::write(policy.join("source"), "synthetic settings\n").unwrap();
    fs::write(policy.join("files-device.json"), json!({"version":1,"id":"fixture","entries":[{"kind":"file","path":home.join("settings"),"source":"source","mode":"0600","group":"base"}]}).to_string()).unwrap();
    let guard = home.join(".local/bin/synctjik");
    fs::write(&guard, "#!/bin/sh\n[ \"$1\" = protection-migration ] || exit 99\n[ \"$BUSY\" != 1 ] || exit 75\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n").unwrap();
    fs::set_permissions(&guard, fs::Permissions::from_mode(0o755)).unwrap();
    let run = |busy: bool| {
        Command::new(env!("CARGO_BIN_EXE_tjikup"))
            .args(["workflow", "--repo"])
            .arg(root)
            .args(["files", "home-files", "apply"])
            .env("HOME", &home)
            .env("XDG_STATE_HOME", root.join("state"))
            .env("BUSY", if busy { "1" } else { "0" })
            .output()
            .unwrap()
    };
    let busy = run(true);
    assert_eq!(
        busy.status.code(),
        Some(75),
        "{}",
        String::from_utf8_lossy(&busy.stderr)
    );
    assert!(!home.join("settings").exists());
    let idle = run(false);
    assert!(
        idle.status.success(),
        "{}",
        String::from_utf8_lossy(&idle.stderr)
    );
    assert_eq!(
        fs::read_to_string(home.join("settings")).unwrap(),
        "synthetic settings\n"
    );
}
