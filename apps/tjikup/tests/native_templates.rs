use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
fn run(root: &Path, recipes: Value) -> Output {
    fs::write(
        root.join("config.json"),
        serde_json::to_vec(&json!({"version":2,"recipes":recipes})).unwrap(),
    )
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_tjikup"))
        .args(["--config", root.join("config.json").to_str().unwrap()])
        .env("XDG_STATE_HOME", root.join("state"))
        .env("TJIKUP_NOTIFY", "0")
        .output()
        .unwrap()
}
fn batch(templates: Value) -> Value {
    json!({"id":"render","render":{"native":"{config_dir}/inputs.json","templates":templates}})
}
fn template(name: &str) -> Value {
    json!({"name":name,"source":format!("{{config_dir}}/{name}.j2"),"syntax":["sh","-n","{script}"]})
}
#[test]
fn native_context_includes_and_dependent_execution_need_no_manager() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(
        root.join("data.json"),
        r#"{"client":true,"items":["a","b"]}"#,
    )
    .unwrap();
    fs::write(root.join("helper"), "# helper\n").unwrap();
    fs::write(
        root.join("inputs.json"),
        r#"{"version":1,"context_files":["data.json"],"includes":{"helper":"helper"}}"#,
    )
    .unwrap();
    fs::write(root.join("task.sh.j2"), "{{ include('helper') }}{% if client %}{% for item in items %}printf '%s\\n' '{{ item }}'\n{% endfor %}{% endif %}").unwrap();
    let output = run(
        root,
        json!([batch(json!([template("task.sh")])), {"id":"use","needs":["render"],"apply":["sh","{run_dir}/task.sh"]}]),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let log = fs::read_dir(root.join("state/tjikup/logs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(fs::read_to_string(log.join("use.log"))
        .unwrap()
        .contains("a\nb\n"));
}
#[test]
fn failed_batch_publishes_nothing_and_does_not_echo_sensitive_source() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(
        root.join("inputs.json"),
        r#"{"version":1,"context":{"value":"PRIVATE-MARKER-0123"}}"#,
    )
    .unwrap();
    fs::write(root.join("good.sh.j2"), "true\n").unwrap();
    fs::write(root.join("bad.sh.j2"), "if {{ value }}\n").unwrap();
    let output = run(
        root,
        json!([batch(json!([template("good.sh"),template("bad.sh")])),{"id":"use","needs":["render"],"apply":["touch","bad"]}]),
    );
    assert!(!output.status.success());
    assert!(!root.join("bad").exists());
    let log = fs::read_dir(root.join("state/tjikup/logs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(!log.join("good.sh").exists());
    assert!(!fs::read_to_string(log.join("render.log"))
        .unwrap()
        .contains("PRIVATE-MARKER"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE-MARKER"));
}
#[test]
fn missing_variables_and_escaping_include_paths_fail_closed() {
    for inputs in [
        r#"{"version":1}"#,
        r#"{"version":1,"includes":{"helper":"../outside"}}"#,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join("inputs.json"), inputs).unwrap();
        fs::write(root.join("task.sh.j2"), "{{ missing }}").unwrap();
        assert!(!run(root, json!([batch(json!([template("task.sh")]))]))
            .status
            .success());
    }
}

#[test]
fn bitwarden_values_are_fish_quoted_without_evaluation_or_provider_logs() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let id = "11111111-1111-1111-1111-111111111111";
    let value = "quote' slash\\ dollar$ (touch injected)\nsecond line";
    fs::write(root.join("token"), "BWS_ACCESS_TOKEN=SYNTHETIC\n").unwrap();
    fs::set_permissions(root.join("token"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(root.join("provider.py"),format!("import json,sys\nprint('PRIVATE-PROVIDER-STDERR',file=sys.stderr)\nprint(json.dumps({}))\n",json!({"id":id,"value":value}))).unwrap();
    fs::write(root.join("inputs.json"),serde_json::to_vec(&json!({"version":1,
        "context":{"destination":root.join("result")},
        "bitwarden":{"command":["python3",root.join("provider.py")],"access_token_file":root.join("token")}})).unwrap()).unwrap();
    fs::write(root.join("task.fish.j2"),format!("printf '%s' {{{{ secret('{id}') | fish_quote }}}} > {{{{ destination | fish_quote }}}}\n")).unwrap();
    let output = run(
        root,
        json!([batch(json!([{"name":"task.fish","source":"{config_dir}/task.fish.j2","syntax":["fish","--no-config","--no-execute","{script}"]}])),
        {"id":"use","needs":["render"],"apply":["fish","--no-config","{run_dir}/task.fish"]}]),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(root.join("result")).unwrap(), value);
    assert!(!root.join("injected").exists());
    let log = fs::read_dir(root.join("state/tjikup/logs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    for file in ["render.log", "use.log"] {
        let text = fs::read_to_string(log.join(file)).unwrap();
        assert!(!text.contains("PRIVATE-PROVIDER"));
        assert!(!text.contains(value));
    }
}

#[test]
fn preparation_reports_recipe_count_without_show_skips() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(root.join("inputs.json"), r#"{"version":1}"#).unwrap();
    for name in ["one.sh", "two.sh"] {
        fs::write(root.join(format!("{name}.j2")), "true\n").unwrap();
    }
    let mut recipe = batch(json!([template("one.sh"), template("two.sh")]));
    recipe["label"] = json!("Maintenance scripts (home)");
    let result = run(root, json!([recipe]));
    assert!(result.status.success());
    let output = String::from_utf8_lossy(&result.stderr);
    assert!(output.contains("2 recipes prepared"), "{output}");
}
