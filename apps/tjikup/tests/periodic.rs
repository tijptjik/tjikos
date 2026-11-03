use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn config(command: &str) -> Value {
    json!({"version":2,"recipes":[{"id":"cleanup","label":"Monthly cleanup",
        "interval_seconds":2592000,"apply":["sh","-c",command]}]})
}
fn run(root: &Path, config: &Value, args: &[&str]) -> Output {
    fs::write(
        root.join("config.json"),
        serde_json::to_vec(config).unwrap(),
    )
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_tjikup"))
        .args(["--config"])
        .arg(root.join("config.json"))
        .args(args)
        .env("HOME", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("TJIKUP_NOTIFY", "0")
        .current_dir(root)
        .output()
        .unwrap()
}
fn receipt(root: &Path) -> std::path::PathBuf {
    root.join("state/tjikup/periodic/cleanup.json")
}
fn ok(output: Output) {
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn first_success_skips_until_due_then_runs_again() {
    let root = tempfile::tempdir().unwrap();
    let policy = config("echo run >> executions");
    ok(run(root.path(), &policy, &[]));
    let saved = fs::read(receipt(root.path())).unwrap();
    ok(run(root.path(), &policy, &["--show-skips"]));
    assert_eq!(
        fs::read_to_string(root.path().join("executions")).unwrap(),
        "run\n"
    );
    assert_eq!(fs::read(receipt(root.path())).unwrap(), saved);
    let mut old: Value = serde_json::from_slice(&saved).unwrap();
    old["completed_at"] = json!(old["completed_at"].as_u64().unwrap() - 2592001);
    fs::write(receipt(root.path()), serde_json::to_vec(&old).unwrap()).unwrap();
    ok(run(root.path(), &policy, &[]));
    assert_eq!(
        fs::read_to_string(root.path().join("executions")).unwrap(),
        "run\nrun\n"
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(receipt(root.path()))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn failures_deferrals_and_reported_failures_never_advance_cadence() {
    for (command, skipped) in [
        ("exit 23", false),
        ("exit 75", true),
        ("echo 'FAIL    ✗ Cleanup  synthetic failure'", false),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut policy = config(command);
        policy["recipes"][0]["skip_codes"] = json!([75]);
        let result = run(root.path(), &policy, &[]);
        assert_eq!(result.status.success(), skipped, "{result:?}");
        assert!(!receipt(root.path()).exists());
        ok(run(root.path(), &config("touch retried"), &[]));
        assert!(root.path().join("retried").exists());
    }
}

#[test]
fn preview_and_invalid_intervals_never_execute_or_create_state() {
    let root = tempfile::tempdir().unwrap();
    for mode in ["--plan", "--check", "--dry-run"] {
        ok(run(root.path(), &config("touch unexpected"), &[mode]));
    }
    for seconds in [0, u64::MAX] {
        let mut policy = config("touch unexpected");
        policy["recipes"][0]["interval_seconds"] = json!(seconds);
        assert!(!run(root.path(), &policy, &[]).status.success());
    }
    assert!(!root.path().join("state").exists());
    assert!(!root.path().join("unexpected").exists());
}

#[test]
fn malformed_or_future_receipts_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    ok(run(root.path(), &config("true"), &[]));
    for value in [
        "not json",
        "{\"version\":1,\"completed_at\":18446744073709551615}",
    ] {
        fs::write(receipt(root.path()), value).unwrap();
        assert!(!run(root.path(), &config("touch unexpected"), &[])
            .status
            .success());
        assert!(!root.path().join("unexpected").exists());
    }
}

#[test]
fn shared_recipe_lock_blocks_a_second_config() {
    use std::os::fd::AsRawFd;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("state/tjikup/periodic");
    fs::create_dir_all(&directory).unwrap();
    let lock = fs::File::create(directory.join("cleanup.lock")).unwrap();
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    assert!(!run(root.path(), &config("touch unexpected"), &[])
        .status
        .success());
    assert!(!root.path().join("unexpected").exists());
    assert!(!receipt(root.path()).exists());
    drop(lock);
    ok(run(root.path(), &config("touch expected"), &[]));
    assert!(root.path().join("expected").exists());
}

#[test]
fn timeout_keeps_cleanup_due() {
    let root = tempfile::tempdir().unwrap();
    let mut policy = config("sleep 10");
    policy["recipes"][0]["timeout"] = json!(1);
    assert!(!run(root.path(), &policy, &[]).status.success());
    assert!(!receipt(root.path()).exists());
    ok(run(root.path(), &config("touch retried"), &[]));
    assert!(root.path().join("retried").exists());
}
