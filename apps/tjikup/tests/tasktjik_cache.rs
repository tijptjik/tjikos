use std::{fs, process::Command};

#[test]
fn tasktjik_install_cache_reuses_and_invalidates_artifacts() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("source");
    let output = root.path().join("installed");
    fs::write(&input, "first").unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_tjikup"))
            .args(["workflow", "cache-install", "tasktjik", "--input"])
            .arg(&input)
            .arg("--output")
            .arg(&output)
            .args(["--", "cp"])
            .arg(&input)
            .arg(&output)
            .env("XDG_STATE_HOME", root.path().join("state"))
            .output()
            .unwrap()
    };
    let initial = run();
    assert!(initial.status.success(), "{:?}", initial);
    assert_eq!(fs::read_to_string(&output).unwrap(), "first");
    let cached = run();
    assert!(cached.status.success());
    assert!(String::from_utf8_lossy(&cached.stdout).contains("SKIP"));
    fs::write(&input, "second").unwrap();
    assert!(run().status.success());
    assert_eq!(fs::read_to_string(&output).unwrap(), "second");
}

#[test]
fn reports_tasktjik_cache_capability() {
    let result = Command::new(env!("CARGO_BIN_EXE_tjikup"))
        .arg("capabilities")
        .output()
        .unwrap();
    assert!(result.status.success());
    let capabilities: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(capabilities["tasktjik_install_cache"], 1);
}
