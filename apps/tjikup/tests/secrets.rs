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
    fn enable_cache(&mut self) {
        let p = self.dir.path();
        fs::write(p.join("keyring.py"), "import sys\nfrom pathlib import Path\np=Path(__file__).parent/'key'\nif sys.argv[1]=='lookup':\n    if not p.exists():sys.exit(1)\n    print(p.read_text())\nelif sys.argv[1]=='store':\n    p.write_text(sys.stdin.read())\nelse:sys.exit(2)\n").unwrap();
        self.manifest["bitwarden"]["cache"] =
            json!({"directory":p.join("cache"),"keyring_command":["python3",p.join("keyring.py")]});
    }
    fn calls(&self) -> usize {
        fs::read_to_string(self.dir.path().join("calls"))
            .unwrap()
            .lines()
            .count()
    }
    fn encrypted(&self) -> Vec<std::path::PathBuf> {
        fs::read_dir(self.dir.path().join("cache"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "enc"))
            .collect()
    }
    fn run(&self, mode: &str) -> Output {
        self.run_timed(mode, false)
    }
    fn run_timed(&self, mode: &str, timed: bool) -> Output {
        self.run_refresh(mode, timed, "", "")
    }
    fn run_refresh(&self, mode: &str, timed: bool, refresh: &str, generation: &str) -> Output {
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
            .env("TJIKUP_REFRESH_SECRETS", refresh)
            .env("TJIKUP_SECRET_REFRESH_RUN", generation)
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

#[test]
fn encrypted_cache_survives_processes_and_refreshes_only_when_requested() {
    let mut f = Fixture::new(&format!("{{{{ secret('{ID}') }}}}"));
    f.enable_cache();
    ok(&f.run("apply"));
    assert_eq!(f.calls(), 1);
    let files = f.encrypted();
    assert_eq!(files.len(), 1);
    let bytes = fs::read(&files[0]).unwrap();
    for private in ["SYNTHETIC-SECRET", "SYNTHETIC-TOKEN", ID] {
        assert!(!bytes
            .windows(private.len())
            .any(|w| w == private.as_bytes()));
    }
    assert_eq!(
        fs::metadata(&files[0]).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::write(f.dir.path().join("value"), "ROTATED-SECRET").unwrap();
    let warm = f.run_timed("apply", true);
    ok(&warm);
    assert_eq!(f.calls(), 1);
    assert!(String::from_utf8_lossy(&warm.stderr).contains("0 requests"));
    assert_eq!(
        fs::read_to_string(f.dir.path().join("root/secret")).unwrap(),
        "SYNTHETIC-SECRET"
    );
    ok(&f.run_refresh("apply", true, ID, "refresh-one"));
    assert_eq!(f.calls(), 2);
    assert_eq!(
        fs::read_to_string(f.dir.path().join("root/secret")).unwrap(),
        "ROTATED-SECRET"
    );
    ok(&f.run_refresh("apply", false, ID, "refresh-one"));
    assert_eq!(f.calls(), 2, "same refresh wave is deduplicated");
    ok(&f.run_refresh("apply", false, "all", "refresh-two"));
    assert_eq!(f.calls(), 3);
}
#[test]
fn corrupt_ciphertext_refetches_but_never_substitutes_unverified_plaintext() {
    let mut f = Fixture::new(&format!("{{{{ secret('{ID}') }}}}"));
    f.enable_cache();
    ok(&f.run("apply"));
    let file = f.encrypted().remove(0);
    let mut bytes = fs::read(&file).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(&file, &bytes).unwrap();
    ok(&f.run("apply"));
    assert_eq!(f.calls(), 2);
    fs::write(&file, b"SYNTHETIC-UNENCRYPTED-ATTACK").unwrap();
    fs::write(f.dir.path().join("provider.py"), "import sys;sys.exit(1)").unwrap();
    assert_eq!(f.run("apply").status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(f.dir.path().join("root/secret")).unwrap(),
        "SYNTHETIC-SECRET"
    );
}
#[test]
fn rotated_access_token_cannot_reuse_previous_auth_scope() {
    let mut f = Fixture::new(&format!("{{{{ secret('{ID}') }}}}"));
    f.enable_cache();
    ok(&f.run("apply"));
    let p = f.dir.path();
    fs::write(p.join("credential"), "BWS_ACCESS_TOKEN=ROTATED-TOKEN\n").unwrap();
    let provider = fs::read_to_string(p.join("provider.py"))
        .unwrap()
        .replace("SYNTHETIC-TOKEN", "ROTATED-TOKEN");
    fs::write(p.join("provider.py"), provider).unwrap();
    ok(&f.run("apply"));
    assert_eq!(f.calls(), 2);
    assert_eq!(f.encrypted().len(), 2);
}
#[test]
fn unavailable_keyring_uses_provider_without_plaintext_cache() {
    let mut f = Fixture::new(&format!("{{{{ secret('{ID}') }}}}"));
    f.enable_cache();
    fs::write(f.dir.path().join("keyring.py"), "import sys;sys.exit(1)").unwrap();
    let result = f.run("apply");
    ok(&result);
    assert!(String::from_utf8_lossy(&result.stderr).contains("no plaintext cache"));
    ok(&f.run("apply"));
    assert_eq!(f.calls(), 2);
    assert!(f.encrypted().is_empty());
}
#[test]
fn symlink_cache_entry_is_rejected_without_following_it() {
    use std::os::unix::fs::symlink;
    let mut f = Fixture::new(&format!("{{{{ secret('{ID}') }}}}"));
    f.enable_cache();
    ok(&f.run("apply"));
    let file = f.encrypted().remove(0);
    fs::remove_file(&file).unwrap();
    let target = f.dir.path().join("unrelated");
    fs::write(&target, "KEEP").unwrap();
    symlink(&target, &file).unwrap();
    assert_eq!(f.run("apply").status.code(), Some(2));
    assert_eq!(fs::read_to_string(target).unwrap(), "KEEP");
}
#[test]
fn concurrent_readers_fetch_a_missing_secret_once() {
    let mut f = Fixture::new(&format!("{{{{ secret('{ID}') }}}}"));
    f.enable_cache();
    let p = f.dir.path();
    fs::write(
        p.join("manifest.json"),
        serde_json::to_vec(&f.manifest).unwrap(),
    )
    .unwrap();
    let mut children = Vec::new();
    for _ in 0..3 {
        children.push(
            Command::new(env!("CARGO_BIN_EXE_tjikup"))
                .args(["files", "plan", "--manifest"])
                .arg(p.join("manifest.json"))
                .arg("--root")
                .arg(p.join("root"))
                .arg("--state-dir")
                .arg(p.join("state"))
                .env_remove("BWS_ACCESS_TOKEN")
                .env_remove("TJIKUP_REFRESH_SECRETS")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
    }
    for mut child in children {
        assert_eq!(child.wait().unwrap().code(), Some(1));
    }
    assert_eq!(f.calls(), 1);
    assert_eq!(f.encrypted().len(), 1);
}
