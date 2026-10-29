use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

#[test]
fn integrations_do_not_load_archive_policy_or_execute_backups_for_checks() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let bin = home.join("bin");
    fs::create_dir(&bin).unwrap();
    for command in ["rclone", "restic"] {
        let path = bin.join(command);
        fs::write(
            &path,
            "#!/bin/sh\necho unexpected dependency execution >&2\nexit 99\n",
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let policy = home.join("cloudflare.json");
    fs::write(&policy, serde_json::to_vec(&json!({
        "mount_uuid":"fixture", "mount_point":home.join("disk"),
        "repository":home.join("disk/BACKUP/restic"), "account_id":"fixture-account",
        "restic_password":home.join("password"), "cloudflare_token":home.join("api-token"),
        "cloudflare_config_token":home.join("config-token"), "rclone_config":home.join("rclone.conf"),
        "r2_remote":"fixture:", "expected_r2_buckets":1
    })).unwrap()).unwrap();
    for args in [
        vec!["capabilities"],
        vec!["cloudflare", "--help"],
        vec!["private-assets", "--help"],
        vec!["cloudflare", "check", "--policy", policy.to_str().unwrap()],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_synctjik"))
            .args(&args)
            .env("HOME", home)
            .env(
                "PATH",
                format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
            )
            .output()
            .unwrap();
        if args == ["capabilities"] {
            let capabilities: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(capabilities["native_backends"], 1);
            assert_eq!(capabilities["dependencies"], json!(["rclone", "restic"]));
        }
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let failed = Command::new(env!("CARGO_BIN_EXE_synctjik"))
        .args([
            "cloudflare",
            "check",
            "--policy",
            home.join("missing.json").to_str().unwrap(),
        ])
        .env("HOME", home)
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(!home.join(".local").exists());
    assert!(!home.join("disk").exists());
    assert!(!home.join(".config").exists());
}
