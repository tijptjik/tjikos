use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn fixture(recipes: Value) -> (TempDir, Value) {
    (
        tempfile::tempdir().unwrap(),
        json!({"version":2,"recipes":recipes}),
    )
}
fn invoke(dir: &Path, config: &Value, args: &[&str]) -> Output {
    fs::write(dir.join("config.json"), serde_json::to_vec(config).unwrap()).unwrap();
    Command::new(env!("CARGO_BIN_EXE_tjikup"))
        .args(["--config", dir.join("config.json").to_str().unwrap()])
        .args(args)
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("TJIKUP_NOTIFY", "0")
        .output()
        .unwrap()
}
fn ok(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn receipt(dir: &Path) -> Value {
    let entry = fs::read_dir(dir.join("state/tjikup/logs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    serde_json::from_slice(&fs::read(entry.join("receipt.json")).unwrap()).unwrap()
}
fn script(dir: &Path, name: &str, content: &str) -> Value {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    json!(["sh", path])
}

#[test]
fn failed_command_reports_exit_and_log_even_after_success_status() {
    let (dir, config) = fixture(json!([
        {"id":"install","label":"Synthetic installer","apply":["sh","-c","echo 'UPDATE  ✓ Earlier step  completed'; echo private-diagnostic >&2; exit 23"]}
    ]));
    let output = invoke(dir.path(), &config, &[]);
    assert!(!output.status.success());
    let terminal = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(terminal.contains("FAIL"), "{terminal}");
    assert!(terminal.contains("apply exited 23"), "{terminal}");
    assert!(
        terminal.contains("Failed: Synthetic installer (install)"),
        "{terminal}"
    );
    let r = receipt(dir.path());
    let log = r["recipes"]["install"]["log"].as_str().unwrap();
    assert!(terminal.contains(log), "{terminal}");
    assert!(!terminal.contains("private-diagnostic"));
    assert!(fs::read_to_string(log)
        .unwrap()
        .contains("private-diagnostic"));
}

#[test]
fn validates_whole_graph_before_any_command() {
    for recipes in [
        json!([{"id":"a","apply":["touch","bad"],"needs":["missing"]}]),
        json!([{"id":"a","apply":["true"],"needs":["b"]},{"id":"b","apply":["true"],"needs":["a"]}]),
        json!([{"id":"a","apply":["true"],"typo":true}]),
    ] {
        let (dir, config) = fixture(recipes);
        assert!(!invoke(dir.path(), &config, &[]).status.success());
        assert!(!dir.path().join("bad").exists());
        assert!(!dir.path().join("state").exists());
    }
}

#[test]
fn checks_skip_apply_and_satisfy_dependents() {
    let (dir, config) = fixture(json!([
        {"id":"current","check":["true"],"apply":["touch","bad"]},
        {"id":"dependent","needs":["current"],"check":["false"],"apply":["touch","good"]}
    ]));
    ok(&invoke(dir.path(), &config, &[]));
    assert!(!dir.path().join("bad").exists());
    assert!(dir.path().join("good").exists());
    let r = receipt(dir.path());
    assert_eq!(r["recipes"]["current"]["state"], "unchanged");
}

#[test]
fn failed_checks_and_prerequisites_never_apply() {
    let (dir, mut config) =
        fixture(json!([{"id":"apply","check":["sh","-c","exit 2"],"apply":["touch","bad"]}]));
    assert!(!invoke(dir.path(), &config, &[]).status.success());
    assert!(!dir.path().join("bad").exists());
    config["recipes"] = json!([{"id":"apply","apply":["touch","bad"]}]);
    config["prerequisites"] = json!([{"id":"required","check":["false"]}]);
    let output = invoke(dir.path(), &config, &[]);
    assert!(!output.status.success());
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("REQUIRE") && text.contains("Prerequisites"));
    assert!(!text.contains("Receipt:"));
    assert!(!dir.path().join("bad").exists());
}

#[test]
fn plan_and_check_execute_nothing_and_create_no_state() {
    let (dir, config) = fixture(
        json!([{"id":"a","check":["touch","check"],"apply":["touch","apply"],"preview":["touch","preview"]}]),
    );
    for mode in ["--plan", "--dry-run", "--check"] {
        ok(&invoke(dir.path(), &config, &[mode]));
    }
    for file in ["check", "apply", "preview", "state"] {
        assert!(!dir.path().join(file).exists());
    }
}

#[test]
fn failure_drains_peers_but_blocks_new_work() {
    let (dir, mut config) = fixture(json!([]));
    let peer = script(dir.path(), "peer.sh", "sleep 0.2\ntouch drained\n");
    config["jobs"] = json!(2);
    config["recipes"] = json!([
        {"id":"failure","apply":["false"]},
        {"id":"peer","apply":peer},
        {"id":"later","needs":["peer"],"apply":["touch","bad"]}
    ]);
    assert!(!invoke(dir.path(), &config, &[]).status.success());
    assert!(dir.path().join("drained").exists());
    assert!(!dir.path().join("bad").exists());
    assert_eq!(receipt(dir.path())["recipes"]["later"]["state"], "blocked");
}

#[test]
fn checks_and_apply_share_a_resource_lease() {
    let (dir, mut config) = fixture(json!([]));
    let check = script(
        dir.path(),
        "check.sh",
        "mkdir lease || exit 2\nsleep 0.05\nexit 1\n",
    );
    let apply = script(
        dir.path(),
        "apply.sh",
        "test -d lease || exit 2\nsleep 0.05\nrmdir lease\n",
    );
    config["recipes"] = json!([
        {"id":"a","resources":["packages"],"check":check,"apply":apply},
        {"id":"b","resources":["packages"],"check":check,"apply":apply}
    ]);
    ok(&invoke(dir.path(), &config, &[]));
    assert!(
        receipt(dir.path())["recipes"]["b"]["resource_wait"]
            .as_f64()
            .unwrap()
            > 0.05
    );
}

#[test]
fn timeout_kills_descendants() {
    let (dir, mut config) = fixture(json!([]));
    let child = script(dir.path(), "slow.sh", "(sleep 2; touch escaped) &\nwait\n");
    config["recipes"] = json!([{"id":"slow","apply":child,"timeout":1}]);
    assert!(!invoke(dir.path(), &config, &[]).status.success());
    std::thread::sleep(std::time::Duration::from_millis(1200));
    assert!(!dir.path().join("escaped").exists());
}

#[test]
fn interrupts_stop_dispatch_and_kill_owned_groups() {
    let (dir, mut config) = fixture(json!([]));
    let child = script(
        dir.path(),
        "slow.sh",
        "touch started\n(sleep 2; touch escaped) &\nwait\n",
    );
    config["recipes"] = json!([{"id":"slow","apply":child},{"id":"later","needs":["slow"],"apply":["touch","bad"]}]);
    fs::write(
        dir.path().join("config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let mut process = Command::new(env!("CARGO_BIN_EXE_tjikup"))
        .args(["--config", dir.path().join("config.json").to_str().unwrap()])
        .env("TJIKUP_NOTIFY", "0")
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if dir.path().join("started").exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(dir.path().join("started").exists());
    unsafe {
        libc::kill(process.id() as i32, libc::SIGINT);
    }
    assert_eq!(process.wait().unwrap().code(), Some(130));
    std::thread::sleep(std::time::Duration::from_millis(2100));
    assert!(!dir.path().join("escaped").exists());
    assert!(!dir.path().join("bad").exists());
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    ok(&output);
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn autocommit_preserves_unrelated_staged_changes_and_literal_paths() {
    let (dir, config) = fixture(
        json!([{"id":"capture-commit","autocommit":{"repository":"{config_dir}","files":["selected[1]"],"message":"capture"}}]),
    );
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.name", "Fixture"]);
    git(
        dir.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    for name in ["selected[1]", "selected1", "other"] {
        fs::write(dir.path().join(name), "before").unwrap();
    }
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "initial"]);
    for name in ["selected[1]", "selected1", "other"] {
        fs::write(dir.path().join(name), "after").unwrap();
    }
    git(dir.path(), &["add", "other"]);
    ok(&invoke(dir.path(), &config, &[]));
    assert_eq!(git(dir.path(), &["show", "HEAD:selected[1]"]), "after");
    assert_eq!(git(dir.path(), &["show", "HEAD:selected1"]), "before");
    assert_eq!(git(dir.path(), &["show", "HEAD:other"]), "before");
    assert_eq!(
        git(dir.path(), &["diff", "--cached", "--name-only"]),
        "other\n"
    );
}

#[test]
fn untracked_capture_paths_explain_failure_without_mutating_index_or_head() {
    let (dir, config) = fixture(json!([
        {"id":"capture-commit","label":"Commit captured settings","autocommit":{
            "repository":"{config_dir}","files":["tracked", "new[1]", "missing"],"message":"capture"}},
        {"id":"dependent","needs":["capture-commit"],"apply":["touch","should-not-run"]}
    ]));
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.name", "Fixture"]);
    git(
        dir.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    for name in ["tracked", "other"] {
        fs::write(dir.path().join(name), "before").unwrap();
    }
    git(dir.path(), &["add", "tracked", "other"]);
    git(dir.path(), &["commit", "-qm", "initial"]);
    for name in ["tracked", "other", "new[1]"] {
        fs::write(dir.path().join(name), "after").unwrap();
    }
    git(dir.path(), &["add", "other"]);
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let index = fs::read(dir.path().join(".git/index")).unwrap();
    let output = invoke(dir.path(), &config, &[]);
    assert!(!output.status.success());
    assert_eq!(index, fs::read(dir.path().join(".git/index")).unwrap());
    assert_eq!(head, git(dir.path(), &["rev-parse", "HEAD"]));
    assert_eq!(
        fs::read_to_string(dir.path().join("new[1]")).unwrap(),
        "after"
    );
    assert!(!dir.path().join("should-not-run").exists());
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("2 untracked; commit first"), "{text}");
    assert!(
        !text.contains("new[1]"),
        "paths should stay in private receipts"
    );
    let r = receipt(dir.path());
    let error = r["recipes"]["capture-commit"]["error"].as_str().unwrap();
    assert!(
        error.contains("new[1]") && error.contains("missing"),
        "{error}"
    );
    assert_eq!(r["recipes"]["dependent"]["state"], "blocked");
}

#[test]
fn invalid_render_blocks_entire_batch_and_removes_rendered_secrets() {
    let (dir, mut config) = fixture(json!([]));
    fs::write(dir.path().join("bad.sh"), "if then\n").unwrap();
    config["managers"] = json!({"home":{"command":["false"],"source":"{config_dir}"}});
    config["recipes"] = json!([
        {"id":"render","render":{"manager":"home","templates":[{"name":"task.sh","source":"{config_dir}/bad.sh","syntax":["sh","-n","{script}"]}]}},
        {"id":"task","needs":["render"],"apply":["touch","bad"]}
    ]);
    assert!(!invoke(dir.path(), &config, &[]).status.success());
    assert!(!dir.path().join("bad").exists());
    let log = receipt(dir.path())["recipes"]["render"]["log"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        fs::read_dir(Path::new(&log).parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("prepared-"))
            .count(),
        0
    );
}

#[test]
fn changed_recipe_config_requires_a_new_run() {
    let (dir, config) = fixture(json!([
        {"id":"edit","apply":["sh","-c","echo ' ' >> config.json"]},
        {"id":"later","needs":["edit"],"apply":["touch","bad"]}
    ]));
    assert!(!invoke(dir.path(), &config, &[]).status.success());
    assert!(!dir.path().join("bad").exists());
}

#[test]
fn config_relative_includes_and_flags_expand_without_shell() {
    let (dir, mut config) = fixture(json!([]));
    fs::create_dir(dir.path().join("recipes")).unwrap();
    fs::write(dir.path().join("recipes/tasks.json"), serde_json::to_vec(&json!({"version":2,"recipes":[{"id":"a","apply":["touch","$TARGET"],"cwd":"{config_dir}"}]})).unwrap()).unwrap();
    config["env"] = json!({"TARGET":"$PREFIX-file", "PREFIX":"example"});
    config["includes"] = json!(["recipes/tasks.json"]);
    ok(&invoke(dir.path(), &config, &[]));
    assert!(dir.path().join("recipes/example-file").exists());
}

#[test]
fn declarative_flags_skip_only_selected_work() {
    let (dir, mut config) = fixture(json!([
        {"id":"sync","when":{"environment":"SKIP_SYNC","equals":"0"},"apply":["touch","synced"]},
        {"id":"apply","needs":["sync"],"apply":["touch","applied"]}
    ]));
    config["flags"] = json!({"skip-repo-sync":"SKIP_SYNC"});
    ok(&invoke(dir.path(), &config, &["--skip-repo-sync"]));
    assert!(!dir.path().join("synced").exists());
    assert!(dir.path().join("applied").exists());
}

#[test]
fn file_apply_excludes_hooks_and_can_answer_multiple_conflicts() {
    let (dir, mut config) = fixture(json!([]));
    let manager = script(dir.path(), "manager.sh", "printf '%s\\n' \"$@\" > arguments\nread a; read b\ntest \"$a\" = skip && test \"$b\" = skip\n");
    config["managers"] =
        json!({"home":{"command":manager,"source":"{config_dir}","skip_conflicts_env":"SKIP"}});
    config["env"] = json!({"SKIP":"1"});
    config["recipes"] = json!([{"id":"files","files":"home"}]);
    ok(&invoke(dir.path(), &config, &[]));
    let argv = fs::read_to_string(dir.path().join("arguments")).unwrap();
    assert!(argv.contains("--exclude=scripts\n"));
    assert!(argv.contains("--interactive=false\n"));
}

#[test]
fn simultaneous_workflow_fails_without_waiting_or_mutating() {
    let (dir, mut config) = fixture(json!([]));
    let action = script(dir.path(), "wait.sh", "touch started\nsleep 1\n");
    config["recipes"] = json!([{"id":"wait","apply":action}]);
    fs::write(
        dir.path().join("config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let mut first = Command::new(env!("CARGO_BIN_EXE_tjikup"))
        .args(["--config", dir.path().join("config.json").to_str().unwrap()])
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .env("TJIKUP_NOTIFY", "0")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if dir.path().join("started").exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = invoke(dir.path(), &config, &[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("busy"));
    assert!(first.wait().unwrap().success());
}

#[test]
fn notification_endpoint_rejects_remote_actions_and_calls_local_notjikctl() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("notjikctl");
    fs::write(&binary, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$TRACE\"\n").unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    let mut payload = json!({"title":"Remote","body":"Ready","data":{"state":"ALL SYSTEMS GO","tone":"success","duration":"1s","results":[{"label":"Completed","value":"1"},{"label":"Issues","value":"0"}]}});
    let invoke = |payload: &Value| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tjikup"))
            .args(["notify", "--stdin"])
            .env("TJIKUP_NOTIFY", "0")
            .env("PATH", dir.path())
            .env("TRACE", dir.path().join("trace"))
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(serde_json::to_string(payload).unwrap().as_bytes())
            .unwrap();
        child.wait().unwrap()
    };
    assert!(invoke(&payload).success());
    assert!(fs::read_to_string(dir.path().join("trace"))
        .unwrap()
        .contains("results-table"));
    fs::remove_file(dir.path().join("trace")).unwrap();
    payload["actions"] = json!([{"command":["touch","bad"]}]);
    assert!(!invoke(&payload).success());
    assert!(!dir.path().join("trace").exists());
}

#[test]
fn git_sync_refuses_dirty_or_diverged_work_without_resetting() {
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin");
    let local = dir.path().join("local");
    let peer = dir.path().join("peer");
    fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "--bare", "-q"]);
    git(
        dir.path(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            local.to_str().unwrap(),
        ],
    );
    {
        let p = &local;
        git(p, &["config", "user.name", "Fixture"]);
        git(p, &["config", "user.email", "fixture@example.invalid"]);
        git(p, &["config", "commit.gpgsign", "false"]);
    }
    fs::write(local.join("file"), "initial").unwrap();
    git(&local, &["add", "file"]);
    git(&local, &["commit", "-qm", "initial"]);
    git(&local, &["push", "-qu", "origin", "HEAD"]);
    let config = json!({"version":2,"recipes":[{"id":"sync","git":{"repository":local,"operation":"pull-ff"}}]});
    fs::write(local.join("file"), "dirty").unwrap();
    assert!(!invoke(dir.path(), &config, &[]).status.success());
    assert_eq!(fs::read_to_string(local.join("file")).unwrap(), "dirty");
    git(&local, &["add", "file"]);
    git(&local, &["commit", "-qm", "local"]);
    git(
        dir.path(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            peer.to_str().unwrap(),
        ],
    );
    git(&peer, &["config", "user.name", "Fixture"]);
    git(&peer, &["config", "user.email", "fixture@example.invalid"]);
    git(&peer, &["config", "commit.gpgsign", "false"]);
    fs::write(peer.join("peer"), "upstream").unwrap();
    git(&peer, &["add", "peer"]);
    git(&peer, &["commit", "-qm", "peer"]);
    git(&peer, &["push", "-q"]);
    let before = git(&local, &["rev-parse", "HEAD"]);
    let output = invoke(dir.path(), &config, &[]);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("diverged"));
    let saved = fs::read_dir(dir.path().join("state/tjikup/logs"))
        .unwrap()
        .map(|entry| fs::read_to_string(entry.unwrap().path().join("receipt.json")).unwrap())
        .any(|text| text.contains("diverged"));
    assert!(
        saved,
        "divergence diagnostic must remain in the private receipt"
    );
    assert_eq!(git(&local, &["rev-parse", "HEAD"]), before);
}

#[test]
fn script_reports_keep_warnings_without_leaking_notes_and_fail_closed() {
    let (dir, mut config) = fixture(json!([]));
    let warn = script(
        dir.path(),
        "warn.sh",
        "printf 'WARN\t!\tPackage\tprivate detail\n' >> \"$TJIKUP_REPORT_FILE\"\n",
    );
    config["recipes"] = json!([{"id":"warn","apply":warn}]);
    ok(&invoke(dir.path(), &config, &[]));
    let result = receipt(dir.path());
    assert_eq!(result["recipes"]["warn"]["warnings"], json!(["Package"]));
    assert!(!result.to_string().contains("private detail"));
    let other = tempfile::tempdir().unwrap();
    let fail = script(
        other.path(),
        "fail.sh",
        "printf 'INSTALL\t✗\tPackage\tprivate detail\n' >> \"$TJIKUP_REPORT_FILE\"\n",
    );
    let config = json!({"version":2,"recipes":[{"id":"reported","apply":fail},{"id":"blocked","needs":["reported"],"apply":["touch","bad"]}]});
    assert!(!invoke(other.path(), &config, &[]).status.success());
    assert!(!other.path().join("bad").exists());
}

#[test]
fn report_ui_hides_unchanged_and_preparation_without_losing_changes() {
    let (dir, mut config) = fixture(json!([]));
    let changed = script(dir.path(), "changed.sh", "printf 'UPDATE\\t✓\\tBartjik\\tupdated\\nSKIP\\t✓\\tBartjik\\tno changes\\n' > \"$TJIKUP_REPORT_FILE\"\n");
    let skipped = script(
        dir.path(),
        "skip.sh",
        "printf 'SKIP\\t✓\\tAssets (fonts)\\tno changes\\n' > \"$TJIKUP_REPORT_FILE\"\n",
    );
    config["recipes"] = json!([
        {"id":"preparation","apply":["true"],"display":{"quiet":true}},
        {"id":"changed","apply":changed,"display":{"reports":true,"section":"Recipes"}},
        {"id":"skipped","apply":skipped,"display":{"reports":true}},
        {"id":"role-guard","apply":["true"],"display":{"reports":true}}
    ]);
    let output = invoke(dir.path(), &config, &[]);
    ok(&output);
    let text = String::from_utf8_lossy(&output.stderr);
    assert_eq!(text.matches("Bartjik").count(), 1, "{text}");
    assert!(text.contains("UPDATE") && text.contains("ALL SYSTEMS GO"));
    assert!(
        !text.contains("SKIP")
            && !text.contains("Assets")
            && !text.contains("preparation")
            && !text.contains("role-guard")
    );
    assert!(!text.contains("Receipt:") && !text.contains("DONE") && !text.contains("RUN"));
    assert_eq!(text.matches("Recipes").count(), 1);
}

#[test]
fn interactive_status_is_deduplicated_and_unterminated_prompt_is_forwarded() {
    let (dir, mut config) = fixture(json!([]));
    let command = script(dir.path(), "interactive.sh", "printf 'SKIP    ✓ Unchanged    no changes\\n'\nprintf 'UPDATE  ✓ Bartjik    updated\\n'\nprintf 'Continue? '\nsleep 0.1\nprintf '\\n'\n");
    config["recipes"] =
        json!([{"id":"interactive","apply":command,"interactive":true,"display":{"reports":true}}]);
    let output = invoke(dir.path(), &config, &[]);
    ok(&output);
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(!text.contains("SKIP"), "{text}");
    assert_eq!(text.matches("Bartjik").count(), 1, "{text}");
    assert!(text.contains("Continue? "), "{text}");
    let verbose = invoke(dir.path(), &config, &["--show-skips"]);
    ok(&verbose);
    let text = String::from_utf8_lossy(&verbose.stderr);
    assert_eq!(text.matches("Unchanged").count(), 1, "{text}");
    assert_eq!(text.matches("Bartjik").count(), 1, "{text}");
    assert!(text.contains("SKIP"));
}

#[test]
fn repository_panel_groups_operations_and_attention_does_not_claim_success() {
    let (dir, mut config) = fixture(json!([]));
    let warning = script(
        dir.path(),
        "warning.sh",
        "printf 'CHECK\\t!\\tSystemd\\tunit failed\\n' > \"$TJIKUP_REPORT_FILE\"\n",
    );
    config["recipes"] = json!([
        {"id":"pull","apply":["true"],"display":{"repository":"dotjik","operation":"pull"}},
        {"id":"push","apply":["true"],"needs":["pull"],"display":{"repository":"dotjik","operation":"push"}},
        {"id":"warning","apply":warning,"needs":["push"],"display":{"reports":true}}
    ]);
    let output = invoke(dir.path(), &config, &[]);
    ok(&output);
    let text = String::from_utf8_lossy(&output.stderr);
    assert_eq!(text.matches("dotjik").count(), 1, "{text}");
    assert!(text.contains("✓ PULL") && text.contains("✓ PUSH"), "{text}");
    assert!(text.contains("NEEDS ATTENTION") && !text.contains("ALL SYSTEMS GO"));
    assert_eq!(text.matches("Systemd").count(), 1);
}

#[test]
fn show_skips_explains_checks_conditions_and_reports_without_changing_execution() {
    let (dir, mut config) = fixture(json!([]));
    let reported = script(dir.path(), "reports.sh", "printf 'SKIP\\t✓\\tAssets (fonts)\\tno changes\\nSKIP\\t-\\tServer services\\tnot applicable\\nUPDATE\\t✓\\tBartjik\\tupdated\\nSKIP\\t✓\\tBartjik\\tno changes\\n' > \"$TJIKUP_REPORT_FILE\"\n");
    config["env"] = json!({"ENABLED":"0"});
    config["prerequisites"] = json!([{"id":"required","check":["true"]}]);
    config["recipes"] = json!([
        {"id":"current","check":["true"],"apply":["touch","bad-current"]},
        {"id":"disabled","when":{"environment":"ENABLED","equals":"1"},"apply":["touch","bad-disabled"]},
        {"id":"reported","apply":reported,"display":{"reports":true}},
        {"id":"silent","apply":["true"],"display":{"reports":true}},
        {"id":"skip-code","apply":["sh","-c","exit 77"],"skip_codes":[77]}
    ]);
    let quiet = invoke(dir.path(), &config, &[]);
    ok(&quiet);
    let quiet_text = String::from_utf8_lossy(&quiet.stderr);
    assert!(!quiet_text.contains("SKIP") && !quiet_text.contains("Assets"));
    let verbose = invoke(dir.path(), &config, &["--show-skips"]);
    ok(&verbose);
    let text = String::from_utf8_lossy(&verbose.stderr);
    for expected in [
        "SKIP",
        "Assets (fonts)",
        "no changes",
        "Server services",
        "not applicable",
        "current",
        "already current",
        "disabled",
        "condition not met",
        "required",
        "satisfied",
        "silent",
        "no status reported",
        "recipe returned a skip status",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert_eq!(
        text.matches("Bartjik").count(),
        1,
        "a later skip must not replace the update: {text}"
    );
    assert!(text.contains("UPDATE") && text.contains("updated") && text.contains("ALL SYSTEMS GO"));
    assert!(!dir.path().join("bad-current").exists() && !dir.path().join("bad-disabled").exists());
    let r = receipt(dir.path());
    assert!(
        !r.to_string().contains("not applicable"),
        "status notes remain private"
    );
}

#[test]
fn show_skips_is_a_builtin_and_composes_with_workflow_flags() {
    let (dir, mut config) = fixture(json!([
        {"id":"pull","when":{"environment":"SKIP_SYNC","equals":"0"},"apply":["touch","bad"],"display":{"repository":"dotjik","operation":"pull"}}
    ]));
    config["flags"] = json!({"skip-repo-sync":"SKIP_SYNC"});
    let output = invoke(dir.path(), &config, &["--show-skips", "--skip-repo-sync"]);
    ok(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("PULL (skipped)"));
    assert!(!dir.path().join("bad").exists());
    config["flags"] = json!({"show-skips":"USER_FLAG"});
    assert!(!invoke(dir.path(), &config, &["--check"]).status.success());
}

#[test]
fn grouped_results_keep_sections_together_across_async_completion() {
    let (dir, mut config) = fixture(json!([
        {"id":"a","apply":["true"],"display":{"section":"Configuration"}},
        {"id":"b","apply":["true"],"needs":["a"],"display":{"section":"Services"}},
        {"id":"c","apply":["true"],"needs":["b"],"display":{"section":"Configuration"}}
    ]));
    config["presentation"] = json!({"grouped_results":true});
    let output = invoke(dir.path(), &config, &[]);
    ok(&output);
    let text = String::from_utf8_lossy(&output.stderr);
    assert_eq!(text.matches("\nConfiguration\n").count(), 1);
    assert_eq!(text.matches("\nServices\n").count(), 1);
    assert!(text.find("Configuration").unwrap() < text.find("Services").unwrap());
}

#[test]
fn blocked_results_keep_groups_without_alphabetic_reordering() {
    let (dir, mut config) = fixture(json!([
        {"id":"fail","label":"Failure","apply":["false"],"display":{"section":"Preparation"}},
        {"id":"z","label":"Zulu","needs":["fail"],"apply":["true"],"display":{"section":"Configuration"}},
        {"id":"s","label":"Service","needs":["fail"],"apply":["true"],"display":{"section":"Services"}},
        {"id":"a","label":"Internal","needs":["fail"],"apply":["true"],"display":{"section":"Configuration"}}
    ]));
    config["presentation"] = json!({"grouped_results":true,"titles":{"Internal":"Alpha"}});
    let output = invoke(dir.path(), &config, &[]);
    assert!(!output.status.success());
    let text = String::from_utf8_lossy(&output.stderr);
    assert_eq!(text.matches("\nConfiguration\n").count(), 1);
    assert!(text.find("Zulu").unwrap() < text.find("Alpha").unwrap());
    assert!(text.find("Zulu").unwrap() < text.find("\nServices\n").unwrap());
}

fn credential_policy(refresh: Value) -> Value {
    json!({"secret_ids":["11111111-2222-3333-4444-555555555555"],"failure_codes":[77],"refresh":refresh,"retry_safe":true})
}
#[test]
fn credential_recovery_retries_once_and_discards_old_failure_status() {
    let (dir, mut config) = fixture(
        json!([{"id":"auth","label":"Authenticated operation","apply":["sh","-c","echo attempt >> calls; if [ ! -e refreshed ]; then echo 'FAIL   ✗ Old attempt'; printf 'FAIL\t✗\tOld attempt\tfailed\n' > \"$$TJIKUP_REPORT_FILE\"; exit 77; fi; echo 'APPLY   ✓ Recovered'"],"credential_recovery":credential_policy(json!(["sh","-c","test \"$$TJIKUP_REFRESH_SECRETS\" = 11111111-2222-3333-4444-555555555555 && test -n \"$$TJIKUP_SECRET_REFRESH_RUN\" && touch refreshed"]))}]),
    );
    ok(&invoke(dir.path(), &config, &[]));
    assert_eq!(
        fs::read_to_string(dir.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    assert_eq!(receipt(dir.path())["recipes"]["auth"]["state"], "done");
    // Generic failure does not trigger refresh.
    config["recipes"][0]["apply"] = json!(["sh", "-c", "exit 1"]);
    config["recipes"][0]["credential_recovery"]["refresh"] = json!(["touch", "must-not-refresh"]);
    assert!(!invoke(dir.path(), &config, &[]).status.success());
    assert!(!dir.path().join("must-not-refresh").exists());
}
#[test]
fn failed_refresh_and_repeated_auth_failure_never_loop() {
    for fail_refresh in [false, true] {
        let refresh = if fail_refresh {
            json!(["sh", "-c", "echo refresh >> refreshes; exit 1"])
        } else {
            json!(["sh", "-c", "echo refresh >> refreshes"])
        };
        let (dir, config) = fixture(
            json!([{"id":"auth","apply":["sh","-c","echo attempt >> calls; exit 77"],"credential_recovery":credential_policy(refresh)}]),
        );
        let out = invoke(dir.path(), &config, &[]);
        assert!(!out.status.success());
        assert_eq!(
            fs::read_to_string(dir.path().join("calls"))
                .unwrap()
                .lines()
                .count(),
            if fail_refresh { 1 } else { 2 }
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("refreshes"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(if fail_refresh {
                "credential refresh failed"
            } else {
                "auth failed after refresh"
            })
        );
    }
}
#[test]
fn credential_recovery_requires_dedicated_codes_and_explicit_retry_safety() {
    for policy in [
        json!({"secret_ids":["11111111-2222-3333-4444-555555555555"],"failure_codes":[1],"refresh":["true"],"retry_safe":true}),
        json!({"secret_ids":["11111111-2222-3333-4444-555555555555"],"failure_codes":[77],"refresh":["true"],"retry_safe":false}),
    ] {
        let (dir, config) = fixture(
            json!([{"id":"auth","apply":["touch","must-not-run"],"credential_recovery":policy}]),
        );
        assert!(!invoke(dir.path(), &config, &[]).status.success());
        assert!(!dir.path().join("must-not-run").exists());
    }
}

#[test]
fn alphabetical_repository_rows_follow_capture_and_sort_by_name() {
    let (dir, config) = fixture(json!([
        {"id":"capture","label":"Captured settings","apply":["true"],
         "display":{"section":"Capture changes"}},
        {"id":"z","label":"Zulu","apply":["true"],"needs":["capture"],
         "display":{"repository":"Zulu","operation":"pull"}},
        {"id":"a","label":"Alpha","apply":["true"],"needs":["capture"],
         "display":{"repository":"Alpha","operation":"pull"}}
    ]));
    let output = invoke(dir.path(), &config, &["--alphabetical"]);
    ok(&output);
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.find("Capture changes").unwrap() < text.find("Repositories").unwrap());
    assert!(text.find("Alpha").unwrap() < text.find("Zulu").unwrap());
    assert_eq!(text.matches("Repositories").count(), 1);
}

#[test]
fn successful_noop_reports_do_not_become_apply_when_hidden() {
    for report in ["SKIP    ✓ Config  current", "__stage_verb__:SKIP"] {
        let (dir, config) =
            fixture(json!([{"id":"config","label":"Config","apply":["printf","%s\n",report]}]));
        let output = invoke(dir.path(), &config, &[]);
        ok(&output);
        assert_eq!(
            receipt(dir.path())["recipes"]["config"]["state"],
            "unchanged"
        );
        assert!(!String::from_utf8_lossy(&output.stderr).contains("APPLY"));
    }
    let (dir, config) = fixture(
        json!([{"id":"changed","apply":["printf","UPDATE  ✓ Config  changed\nSKIP    ✓ Config  current\n"]}]),
    );
    ok(&invoke(dir.path(), &config, &[]));
    assert_eq!(receipt(dir.path())["recipes"]["changed"]["state"], "done");
}

#[test]
fn native_file_apply_reports_unchanged_on_the_second_run() {
    let (dir, mut config) = fixture(json!([]));
    let root = dir.path();
    fs::create_dir(root.join("root")).unwrap();
    fs::write(root.join("source"), "same bytes\n").unwrap();
    fs::write(root.join("manifest.json"), serde_json::to_vec(&json!({"version":1,"id":"config","entries":[{"kind":"file","path":"/target","source":"source","mode":"0600"}]})).unwrap()).unwrap();
    config["recipes"] = json!([{"id":"files","apply":[env!("CARGO_BIN_EXE_tjikup"),"files","apply","--manifest",root.join("manifest.json"),"--state-dir",root.join("files-state"),"--root",root.join("root")]}]);
    let first = invoke(root, &config, &[]);
    if !first.status.success() {
        for entry in fs::read_dir(root.join("state/tjikup/logs")).unwrap() {
            eprintln!(
                "{}",
                fs::read_to_string(entry.unwrap().path().join("files.log")).unwrap()
            );
        }
    }
    ok(&first);
    let output = invoke(root, &config, &["--show-skips"]);
    ok(&output);
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("SKIP") && !text.contains("APPLY"), "{text}");
    assert_eq!(
        fs::read_to_string(root.join("root/target")).unwrap(),
        "same bytes\n"
    );
}
