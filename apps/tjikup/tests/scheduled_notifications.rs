use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};

#[test]
fn saved_failure_and_relay_keep_structured_results_and_stable_identity() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("sync.json");
    fs::write(
        &config,
        json!({"url":"http://localhost", "socket":root.path().join("offline.sock"),
        "device":"synthetic-device", "secret":"synthetic-secret", "name":"Demo", "kind":"desktop"})
        .to_string(),
    )
    .unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
    let state = root.path().join("tjikup/scheduled");
    fs::create_dir_all(&state).unwrap();
    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap();
    let text = format!("Tjikup · {}\nFAILED · 2026-11-01T03:01:46Z\n94/102 stages completed · 328s\nhome:notjik:update: Blocked\nLogs: journalctl --user -u tjikup-weekly.service", host.trim());
    let pending = state.join("latest.json");
    fs::write(
        &pending,
        json!({"text":text,"exit_code":1,"run_id":"retained-run","delivered":false}).to_string(),
    )
    .unwrap();
    let command = || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tjikup"));
        cmd.env("XDG_STATE_HOME", root.path())
            .env("NOTJIK_SYNC_CONFIG", &config)
            .env("NOTJIK_SYNC_STATE", root.path().join("sync"))
            .env_remove("TJIKUP_TELEGRAM_RELAY")
            .arg("scheduled");
        cmd
    };
    for _ in 0..2 {
        let output = command().arg("--notify-only").output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{:?}", output);
    }
    let saved: Value = serde_json::from_slice(&fs::read(pending).unwrap()).unwrap();
    assert_eq!(saved["exit_code"], 1);
    assert_eq!(saved["card"]["data"]["state"], "FAILED");
    assert_eq!(saved["delivered"], true);
    std::env::set_var("NOTJIK_SYNC_STATE", root.path().join("sync"));
    let db = notjik_sync::client::db().unwrap();
    let notices: Vec<Value> = db
        .prepare("SELECT payload FROM outbox ORDER BY seq")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|row| serde_json::from_str::<Value>(&row.unwrap()).unwrap()["notice"].clone())
        .collect();
    assert_eq!(notices.len(), 2);
    assert_eq!(notices[0]["id"], notices[1]["id"]);
    assert_eq!(notices[1]["type"], "results-table");
    assert_eq!(notices[1]["data"]["results"][0]["value"], "94/102");
    assert_eq!(notices[1]["data"]["duration"], "328s");
    assert_eq!(notices[1]["urgency"], "critical");

    let mut child = command()
        .arg("--report-stdin")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            json!({"host":"demo", "text":"Fallback text",
        "run_id":"relay-run", "card":saved["card"]})
            .to_string()
            .as_bytes(),
        )
        .unwrap();
    assert!(child.wait().unwrap().success());
    let relayed: String = db
        .query_row(
            "SELECT payload FROM outbox ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let relayed: Value = serde_json::from_str(&relayed).unwrap();
    assert_eq!(relayed["notice"]["data"], saved["card"]["data"]);
    assert_eq!(relayed["notice"]["run"], "demo:relay-run");
}
