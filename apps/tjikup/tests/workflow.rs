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

#[test]
fn home_file_groups_are_read_from_the_declared_array() {
    let dir = fixture();
    let host = String::from_utf8(Command::new("hostname").output().unwrap().stdout)
        .unwrap()
        .trim()
        .to_owned();
    let profile = dir.path().join("device.json");
    fs::write(
        &profile,
        serde_json::to_vec(
            &json!({"version":1,"hostname":host,"role":"desktop","subscriptions":[]}),
        )
        .unwrap(),
    )
    .unwrap();
    fs::write(
        dir.path().join("tjikup.json"),
        serde_json::to_vec(
            &json!({"version":2,"device_file":profile,"recipes":[{"id":"base","apply":["true"]}]}),
        )
        .unwrap(),
    )
    .unwrap();
    let policy = dir.path().join("native/home");
    fs::create_dir_all(&policy).unwrap();
    fs::write(policy.join("groups.json"), r#"["base","zed"]"#).unwrap();
    fs::write(policy.join("portable-files.json"), r#"[{"group":"base"}]"#).unwrap();
    fs::write(policy.join("source"), "example\n").unwrap();
    let target = dir.path().join("home/example");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(policy.join("files-device.json"), serde_json::to_vec(&json!({"version":1,"id":"fixture","entries":[{"kind":"file","path":target,"source":"source","mode":"0600","group":"base"}]})).unwrap()).unwrap();
    let run = |group: &str| {
        binary()
            .args(["workflow", "--repo"])
            .arg(dir.path())
            .args(["files", "home-files", "plan", "--group", group])
            .env("HOME", dir.path().join("home"))
            .env("XDG_STATE_HOME", dir.path().join("state"))
            .output()
            .unwrap()
    };
    let base = run("base");
    assert_eq!(
        base.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&base.stderr)
    );
    assert!(String::from_utf8_lossy(&base.stdout).contains("CREATE"));
    assert!(!target.exists());
    let unknown = run("unknown");
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown home file group"));
}

#[test]
fn asset_steps_with_no_change_notes_do_not_report_updated() {
    let dir = fixture();
    let bin = dir.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let fish = bin.join("fish");
    fs::write(
        &fish,
        "#!/bin/sh\nprintf 'UPDATE  ✓ Rose Punk icons  no changes\\n'\n",
    )
    .unwrap();
    fs::set_permissions(fish, fs::Permissions::from_mode(0o755)).unwrap();
    let result = binary()
        .args(["workflow", "--repo"])
        .arg(dir.path())
        .args(["assets", "icons", "--run-dir"])
        .arg(dir.path())
        .env("PATH", bin)
        .output()
        .unwrap();
    assert!(result.status.success());
    let text = String::from_utf8_lossy(&result.stdout);
    assert!(
        text.contains("SKIP") && text.contains("current") && !text.contains("updated"),
        "{text}"
    );
}

#[test]
fn rpm_checks_use_fresh_providers_replacements_and_owned_commands() {
    let dir = fixture();
    let profile = dir.path().join("device.json");
    let host = String::from_utf8(Command::new("hostname").output().unwrap().stdout).unwrap();
    fs::write(
        &profile,
        serde_json::to_vec(
            &json!({"version":1,"hostname":host.trim(),"role":"desktop","subscriptions":[]}),
        )
        .unwrap(),
    )
    .unwrap();
    fs::write(
        dir.path().join("tjikup.json"),
        serde_json::to_vec(
            &json!({"version":2,"device_file":profile,"recipes":[{"id":"noop","apply":["true"]}]}),
        )
        .unwrap(),
    )
    .unwrap();
    fs::write(
        dir.path().join("tjikup/package-checks.json"),
        r#"{"rpm":{"packages":"rpm"}}"#,
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("native/tasks")).unwrap();
    fs::create_dir(dir.path().join("bin")).unwrap();
    let rpm = dir.path().join("bin/rpm");
    fs::write(&rpm, "#!/bin/sh\ncase \"$1\" in\n-qa) case \"$3\" in *PROVIDENAME*OBSOLETENAME*) cat \"$INVENTORY\";; *) exit 9;; esac;;\n-qf) test \"$3\" = /usr/bin/sh;;\n*) exit 9;;\nesac\n").unwrap();
    fs::set_permissions(&rpm, fs::Permissions::from_mode(0o755)).unwrap();
    let inventory = dir.path().join("inventory");
    fs::write(&inventory, "fish\nvirtual-provider\nobsolete-name\n").unwrap();
    for (names, expected) in [
        (
            json!(["fish", "virtual-provider", "obsolete-name", "sh"]),
            true,
        ),
        (json!(["fish", "truly-missing-package"]), false),
        (json!(["cat"]), false), // executable, but not owned according to this RPM DB
    ] {
        fs::write(
            dir.path().join("native/tasks/data.json"),
            serde_json::to_vec(&json!({"packages":{"common":{"rpm":names}}})).unwrap(),
        )
        .unwrap();
        let run = || {
            binary()
                .args(["workflow", "--repo"])
                .arg(dir.path())
                .args(["package", "check", "rpm"])
                .env(
                    "PATH",
                    format!("{}:/usr/bin:/bin", dir.path().join("bin").display()),
                )
                .env("INVENTORY", &inventory)
                .output()
                .unwrap()
        };
        let output = run();
        assert_eq!(
            output.status.success(),
            expected,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if expected {
            fs::write(&inventory, "").unwrap();
            assert!(
                !run().status.success(),
                "removal must invalidate presence immediately"
            );
            fs::write(&inventory, "fish\nvirtual-provider\nobsolete-name\n").unwrap();
        }
    }
}
