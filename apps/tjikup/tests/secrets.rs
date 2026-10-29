use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};
use tempfile::TempDir;
const ID: &str = "11111111-2222-3333-4444-555555555555";
struct Fixture {
    dir: TempDir,
    manifest: Value,
}
impl Fixture {
    fn new(template: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        fs::create_dir(p.join("root")).unwrap();
        fs::write(p.join("credential"), "BWS_ACCESS_TOKEN=SYNTHETIC-TOKEN\n").unwrap();
        fs::set_permissions(p.join("credential"), fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(p.join("source"), template).unwrap();
        fs::write(p.join("provider.py"),"import json,os,sys\nfrom pathlib import Path\nassert os.environ['BWS_ACCESS_TOKEN'] == 'SYNTHETIC-TOKEN'\nassert 'SYNTHETIC-TOKEN' not in ' '.join(sys.argv)\np=Path(__file__).parent\nwith (p/'calls').open('a') as f: f.write(sys.argv[3]+'\\n')\nprint(json.dumps({'id':sys.argv[3],'value':(p/'value').read_text()}))\n").unwrap();
        fs::write(p.join("value"), "SYNTHETIC-SECRET").unwrap();
        let manifest = json!({"version":1,"id":"secrets","bitwarden":{"command":["python3",p.join("provider.py")],"access_token_file":p.join("credential")},"entries":[{"kind":"file","path":"/secret","source":"source","template":true,"mode":"0600","sensitive":true}]});
        Self { dir, manifest }
    }
    fn run(&self, mode: &str) -> Output {
        self.run_timed(mode, false)
    }
    fn run_timed(&self, mode: &str, timed: bool) -> Output {
        let p = self.dir.path();
        fs::write(
            p.join("manifest.json"),
            serde_json::to_vec(&self.manifest).unwrap(),
        )
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_tjikup"))
            .args(["files", mode, "--manifest"])
            .arg(p.join("manifest.json"))
            .arg("--root")
            .arg(p.join("root"))
            .arg("--state-dir")
            .arg(p.join("state"))
            .env_remove("BWS_ACCESS_TOKEN")
            .env("TJIKUP_SECRET_TIMING", if timed { "1" } else { "0" })
            .output()
            .unwrap()
    }
}
fn ok(out: &Output) {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
#[test]
fn bitwarden_uses_private_file_child_environment_and_per_render_cache() {
    let f = Fixture::new(&format!(
        "{{{{ secret('{ID}') }}}}\n{{{{ secret('{ID}') }}}}"
    ));
    ok(&f.run("apply"));
    assert_eq!(
        fs::read_to_string(f.dir.path().join("root/secret")).unwrap(),
        "SYNTHETIC-SECRET\nSYNTHETIC-SECRET"
    );
    assert_eq!(
        fs::read_to_string(f.dir.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    let output = f.run("plan");
    ok(&output);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("SYNTHETIC-SECRET"));
}
#[test]
fn shell_and_json_interpolation_preserve_quotes_newlines_and_dollar_signs() {
    let f = Fixture::new(&format!(
        "set -gx TEST_SECRET {{{{ secret('{ID}') | fish_quote }}}}\n"
    ));
    let value = "literal ' \\ $HOME (touch should-not-exist)\nsecond line";
    fs::write(f.dir.path().join("value"), value).unwrap();
    ok(&f.run("apply"));
    let result = Command::new("fish")
        .args([
            "--no-config",
            "-c",
            "source $argv[1]; printf %s \"$TEST_SECRET\"",
        ])
        .arg(f.dir.path().join("root/secret"))
        .current_dir(f.dir.path())
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, value.as_bytes());
    assert!(!f.dir.path().join("should-not-exist").exists());
    let f = Fixture::new(&format!("{{{{ secret('{ID}') | json_quote }}}}"));
    fs::write(f.dir.path().join("value"), value).unwrap();
    ok(&f.run("apply"));
    let decoded: String =
        serde_json::from_slice(&fs::read(f.dir.path().join("root/secret")).unwrap()).unwrap();
    assert_eq!(decoded, value);
}
#[test]
fn auth_failure_invalid_json_and_wrong_ids_are_redacted_and_never_apply() {
    for body in [
        "import sys\nprint('SYNTHETIC-SECRET',file=sys.stderr)\nsys.exit(1)",
        "print('SYNTHETIC-SECRET')",
        "print('{\"id\":\"wrong\",\"value\":\"SYNTHETIC-SECRET\"}')",
    ] {
        let f = Fixture::new(&format!("{{{{ secret('{ID}') }}}}"));
        fs::write(f.dir.path().join("provider.py"), body).unwrap();
        let output = f.run("apply");
        assert_eq!(output.status.code(), Some(2));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("SYNTHETIC-SECRET"));
        assert!(!f.dir.path().join("root/secret").exists());
        assert!(!f.dir.path().join("state").exists());
    }
}
#[test]
fn secret_files_require_explicit_sensitive_flag_and_private_mode() {
    let mut f = Fixture::new(&format!("{{{{ secret('{ID}') }}}}"));
    f.manifest["entries"][0]["sensitive"] = json!(false);
    assert_eq!(f.run("apply").status.code(), Some(2));
    f.manifest["entries"][0]["sensitive"] = json!(true);
    f.manifest["entries"][0]["mode"] = json!("0644");
    assert_eq!(f.run("apply").status.code(), Some(2));
}
#[test]
fn insecure_credential_files_and_secret_use_in_destination_are_rejected() {
    let mut f = Fixture::new(&format!("{{{{ secret('{ID}') }}}}"));
    fs::set_permissions(
        f.dir.path().join("credential"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert_eq!(f.run("apply").status.code(), Some(2));
    assert!(!f.dir.path().join("calls").exists());
    f.manifest["entries"][0]["path"] = json!(format!("/{{{{ secret('{ID}') }}}}"));
    assert_eq!(f.run("apply").status.code(), Some(2));
}
#[test]
fn later_plain_file_does_not_inherit_secret_taint() {
    let mut f = Fixture::new(&format!("{{{{ secret('{ID}') }}}}"));
    fs::write(f.dir.path().join("plain"), "public").unwrap();
    f.manifest["entries"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"file","path":"/plain","source":"plain"}));
    ok(&f.run("apply"));
}

#[test]
fn wrong_host_is_rejected_before_contacting_bitwarden() {
    let mut fixture = Fixture::new(&format!("{{{{ secret('{ID}') }}}}"));
    fixture.manifest["host"] = json!("not-this-host.invalid");
    assert_eq!(fixture.run("plan").status.code(), Some(2));
    assert!(!fixture.dir.path().join("calls").exists());
}

#[test]
fn timing_diagnostics_expose_only_counts_and_elapsed_time() {
    let f = Fixture::new(&format!(
        "{{{{ secret('{ID}') }}}} {{{{ secret('{ID}') }}}}"
    ));
    let output = f.run_timed("plan", true);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Bitwarden: 1 requests,"));
    assert!(stderr.contains("provider time"));
    for private in [ID, "SYNTHETIC-TOKEN", "SYNTHETIC-SECRET"] {
        assert!(!stderr.contains(private));
    }
    assert!(!f.dir.path().join("root/secret").exists());
}
