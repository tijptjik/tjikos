use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(dir: &Path, recipes: Value, args: &[&str]) -> Output {
    let config = json!({"version":2,"jobs":4,"recipes":recipes});
    fs::write(
        dir.join("config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_tjikup"))
        .args(["--config", dir.join("config.json").to_str().unwrap()])
        .args(args)
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("TJIKUP_NOTIFY", "0")
        .output()
        .unwrap()
}
fn ok(out: &Output) {
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
fn recipes() -> Value {
    json!([
        {"id":"sync","apply":["touch","sync"]},
        {"id":"prepare","operation":"prepare","after":["sync"],"apply":["touch","prepared"]},
        {"id":"install","operation":"install","check":["test","-f","installed"],"needs":["prepare"],"apply":["touch","installed"]},
        {"id":"home","operation":"configure","scope":"home","needs":["install"],"apply":["touch","home"]},
        {"id":"system","operation":"configure","scope":"system","apply":["touch","system"]},
        {"id":"update","operation":"update","part_of":"install","needs":["prepare"],"apply":["touch","updated"]}
    ])
}
#[test]
fn selectors_combine_without_sync_install_or_unselected_work() {
    for (args, home, system, update) in [
        (vec!["--apply-dotfiles"], true, false, false),
        (vec!["--apply-sysfiles"], false, true, false),
        (vec!["--apply-package-updates"], false, false, true),
        (
            vec!["--apply-dotfiles", "--apply-sysfiles"],
            true,
            true,
            false,
        ),
        (
            vec!["--apply-dotfiles", "--apply-package-updates"],
            true,
            false,
            true,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("installed"), "present").unwrap();
        ok(&run(dir.path(), recipes(), &args));
        for (name, expected) in [
            ("home", home),
            ("system", system),
            ("updated", update),
            ("sync", false),
        ] {
            assert_eq!(dir.path().join(name).exists(), expected, "{args:?} {name}");
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("installed")).unwrap(),
            "present"
        );
        // Only the update requires preparation: gates do not pull installer needs.
        assert_eq!(dir.path().join("prepared").exists(), update);
    }
}
#[test]
fn missing_install_is_a_gate_and_never_mutates() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), recipes(), &["--apply-dotfiles"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not installed"));
    for path in ["sync", "prepared", "installed", "home", "updated"] {
        assert!(!dir.path().join(path).exists());
    }
}
#[test]
fn fast_config_does_not_wait_for_unrelated_install_and_updates_run_last() {
    let dir = tempfile::tempdir().unwrap();
    let graph = json!([
        {"id":"slow","operation":"install","apply":["sh","-c","for i in $$(seq 1 100); do test -f configured && { touch slow; exit 0; }; sleep .02; done; exit 1"]},
        {"id":"app","operation":"install","apply":["touch","installed"]},
        {"id":"config","operation":"configure","scope":"home","needs":["app"],"apply":["sh","-c","test -f installed && touch configured"]},
        {"id":"update","operation":"update","apply":["sh","-c","test -f slow && test -f configured && touch updated"]}
    ]);
    ok(&run(dir.path(), graph, &[]));
    assert!(dir.path().join("updated").exists());
}
#[test]
fn skipping_updates_omits_their_checks_and_runs_install_and_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let mut graph = recipes();
    graph[5]["check"] = json!(["touch", "update-check"]);
    ok(&run(dir.path(), graph, &["--skip-package-updates"]));
    for path in ["sync", "prepared", "installed", "home", "system"] {
        assert!(dir.path().join(path).exists());
    }
    for path in ["updated", "update-check"] {
        assert!(!dir.path().join(path).exists());
    }
}
#[test]
fn selected_plans_never_execute_checks_or_previews() {
    let dir = tempfile::tempdir().unwrap();
    let mut graph = recipes();
    graph[2]["check"] = json!(["touch", "bad"]);
    graph[3]["preview"] = json!(["touch", "bad"]);
    for mode in ["--check", "--plan", "--dry-run"] {
        ok(&run(dir.path(), graph.clone(), &[mode, "--apply-dotfiles"]));
    }
    assert!(!dir.path().join("bad").exists());
    assert!(!dir.path().join("state").exists());
}
#[test]
fn invalid_selection_and_unsafe_dependency_fail_before_execution() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!run(
        dir.path(),
        recipes(),
        &["--skip-package-updates", "--apply-package-updates"]
    )
    .status
    .success());
    let mut graph = recipes();
    graph[3]["needs"] = json!(["sync"]);
    assert!(!run(dir.path(), graph, &["--apply-dotfiles"])
        .status
        .success());
    for graph in [
        json!([{"id":"a","part_of":"a","apply":["true"]}]),
        json!([{"id":"a","part_of":"b","apply":["true"]},{"id":"b","part_of":"a","apply":["true"]}]),
        json!([{"id":"a","operation":"mistyped","apply":["true"]}]),
        json!([{"id":"a","operation":"configure","apply":["true"]}]),
    ] {
        assert!(!run(dir.path(), graph, &["--check"]).status.success());
    }
    assert!(!dir.path().join("state").exists());
}
