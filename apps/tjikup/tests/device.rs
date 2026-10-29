use serde_json::{json, Value};
use std::{
    fs,
    process::{Command, Output},
};
fn hostname() -> String {
    String::from_utf8(Command::new("hostname").output().unwrap().stdout)
        .unwrap()
        .trim()
        .into()
}
fn setup() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("config.json"), serde_json::to_vec(&json!({
        "version":2,"device_file":"{config_dir}/device.json","legacy_hosts":[hostname()],
        "prerequisites":[{"id":"require:runtime","check":["true"]}],
        "recipes":[
            {"id":"base","apply":["true"]},
            {"id":"compiler","optional":true,"apply":["true"]},
            {"id":"editor","optional":true,"roles":["desktop","laptop"],"needs":["base","compiler"],"apply":["touch","editor-ran"]},
            {"id":"server","optional":true,"roles":["server"],"apply":["touch","server-ran"]},
            {"id":"finish","after":["editor","server"],"apply":["true"]}
        ]})).unwrap()).unwrap();
    dir
}
fn run(dir: &tempfile::TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tjikup"))
        .args(args)
        .current_dir(dir.path())
        .env("TJIKUP_CONFIG", dir.path().join("config.json"))
        .env("TJIKUP_NOTIFY", "0")
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .output()
        .unwrap()
}
fn enroll(dir: &tempfile::TempDir, role: &str, subscriptions: &str) -> Output {
    run(
        dir,
        &[
            "new",
            "device",
            "--role",
            role,
            "--subscribe",
            subscriptions,
        ],
    )
}
#[test]
fn enrollment_resolves_dependencies_without_ordering_subscriptions_or_execution() {
    let dir = setup();
    let result = enroll(&dir, "laptop", "editor");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let profile: Value =
        serde_json::from_slice(&fs::read(dir.path().join("device.json")).unwrap()).unwrap();
    assert_eq!(profile["hostname"], hostname());
    assert_eq!(profile["subscriptions"], json!(["editor"]));
    assert!(!dir.path().join("editor-ran").exists());
    let plan = run(&dir, &["--plan"]);
    assert!(plan.status.success());
    let plan = String::from_utf8_lossy(&plan.stdout);
    assert!(plan.contains("compiler ·") && plan.contains("editor ·") && plan.contains("finish ·"));
    assert!(!plan.contains("server ·") && !plan.contains("editor, server"));
    assert!(!enroll(&dir, "server", "server").status.success());
}
#[test]
fn invalid_subscriptions_and_roles_do_not_write_profiles() {
    for (role, sub) in [
        ("server", "editor"),
        ("laptop", "missing"),
        ("tablet", "none"),
        ("desktop", "0"),
        ("desktop", "base"),
    ] {
        let dir = setup();
        assert!(!enroll(&dir, role, sub).status.success(), "{role} {sub}");
        assert!(!dir.path().join("device.json").exists());
    }
}
#[test]
fn empty_selection_keeps_baseline_and_profile_is_bound_to_host() {
    let dir = setup();
    assert!(enroll(&dir, "server", "none").status.success());
    let plan = run(&dir, &["--plan"]);
    assert!(plan.status.success());
    let text = String::from_utf8_lossy(&plan.stdout);
    assert!(text.contains("base ·"));
    assert!(!text.contains("server ·"));
    let path = dir.path().join("device.json");
    let mut device: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    device["hostname"] = json!("not-this-machine.invalid");
    fs::write(path, serde_json::to_vec(&device).unwrap()).unwrap();
    assert!(!run(&dir, &["--check"]).status.success());
}
#[test]
fn unenrolled_workflows_preserve_all_recipes_and_ordering() {
    let dir = setup();
    let output = run(&dir, &["--plan"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("editor ·") && text.contains("server ·"));
    assert!(text.contains("editor, require:runtime, server"));
}

#[test]
fn unknown_unenrolled_host_and_bad_ordering_fail_before_execution() {
    let dir = setup();
    let path = dir.path().join("config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["legacy_hosts"] = json!([]);
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(!run(&dir, &["--plan"]).status.success());
    assert!(enroll(&dir, "laptop", "none").status.success());
    config["recipes"][0]["after"] = json!(["missing"]);
    fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(!run(&dir, &["--check"]).status.success());
    assert!(!dir.path().join("editor-ran").exists());
}

#[test]
fn enrollment_and_loading_share_the_xdg_profile_path() {
    let dir = setup();
    let path = dir.path().join("config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["device_file"] = json!("$XDG_CONFIG_HOME/tjikup/device.json");
    config["legacy_hosts"] = json!([]);
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    for custom in [false, true] {
        let home = dir.path().join(if custom { "custom-home" } else { "home" });
        let xdg = if custom {
            dir.path().join("custom-config")
        } else {
            home.join(".config")
        };
        for args in [
            vec!["new", "device", "--role", "laptop", "--subscribe", "none"],
            vec!["--check"],
        ] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_tjikup"));
            command
                .args(args)
                .env("TJIKUP_CONFIG", &path)
                .env("HOME", &home)
                .env_remove("XDG_CONFIG_HOME");
            if custom {
                command.env("XDG_CONFIG_HOME", &xdg);
            }
            let result = command.output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        assert!(xdg.join("tjikup/device.json").is_file());
    }
}

#[test]
fn companions_follow_subscriptions_and_survive_operation_selection() {
    let dir = setup();
    let path = dir.path().join("config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["recipes"] = json!([
        {"id":"base","operation":"configure","scope":"home","apply":["true"]},
        {"id":"editor","optional":true,"operation":"install","check":["true"],"apply":["touch","installed"]},
        {"id":"editor:config","part_of":"editor","operation":"configure","scope":"home","needs":["editor"],"apply":["touch","configured"]},
        {"id":"editor:update","part_of":"editor","operation":"update","apply":["touch","updated"]}
    ]);
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(enroll(&dir, "laptop", "none").status.success());
    let output = run(&dir, &["--plan"]);
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("editor ·"));
    let mut device: Value =
        serde_json::from_slice(&fs::read(dir.path().join("device.json")).unwrap()).unwrap();
    device["subscriptions"] = json!(["editor"]);
    fs::write(
        dir.path().join("device.json"),
        serde_json::to_vec(&device).unwrap(),
    )
    .unwrap();
    assert!(run(&dir, &["--apply-dotfiles"]).status.success());
    assert!(dir.path().join("configured").exists());
    assert!(!dir.path().join("installed").exists());
    assert!(!dir.path().join("updated").exists());
    assert!(run(&dir, &["--apply-package-updates"]).status.success());
    assert!(dir.path().join("updated").exists());
}

#[test]
fn legacy_roles_filter_execution_even_when_showing_other_devices() {
    let dir = setup();
    let path = dir.path().join("config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["legacy_roles"] = json!({hostname(): "desktop"});
    fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
    let output = run(&dir, &["--show-skips"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("not selected for this device"));
    assert!(dir.path().join("editor-ran").exists());
    assert!(!dir.path().join("server-ran").exists());
    let output = run(&dir, &["--show-skips-and-other-devices"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not selected for this device"));
    assert!(!dir.path().join("server-ran").exists());
}
