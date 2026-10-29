use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

struct Fixture {
    dir: TempDir,
    manifest: Value,
}
impl Fixture {
    fn new(entries: Value) -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("root")).unwrap();
        fs::write(dir.path().join("source"), "hello\n").unwrap();
        Self {
            dir,
            manifest: json!({"version":1,"id":"fixture","context":{"name":"world"},"entries":entries}),
        }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    fn run(&self, operation: &str) -> Output {
        self.run_args(operation, &[])
    }
    fn run_args(&self, operation: &str, args: &[&str]) -> Output {
        fs::write(
            self.path("manifest.json"),
            serde_json::to_vec(&self.manifest).unwrap(),
        )
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_tjikup"))
            .args(["files", operation])
            .args(args)
            .arg("--manifest")
            .arg(self.path("manifest.json"))
            .arg("--root")
            .arg(self.path("root"))
            .arg("--state-dir")
            .arg(self.path("state"))
            .output()
            .unwrap()
    }
    fn receipt(&self) -> PathBuf {
        fs::read_dir(self.path("state"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.is_dir())
            .unwrap()
            .join("receipt.json")
    }
    fn restore(&self, receipt: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tjikup"))
            .args(["files", "restore", "--receipt"])
            .arg(receipt)
            .arg("--root")
            .arg(self.path("root"))
            .arg("--state-dir")
            .arg(self.path("state"))
            .output()
            .unwrap()
    }
}
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn file(path: &str) -> Value {
    json!({"kind":"file","path":path,"source":"source","mode":"0640"})
}

#[test]
fn first_apply_then_true_noop_preserves_inode_mtime_and_state() {
    let f = Fixture::new(json!([file("/etc/example")]));
    assert_eq!(f.run("check").status.code(), Some(1));
    assert!(!f.path("state").exists());
    success(&f.run("apply"));
    let target = f.path("root/etc/example");
    let first = fs::metadata(&target).unwrap();
    let state = fs::metadata(f.path("state/fixture.json")).unwrap();
    success(&f.run("apply"));
    success(&f.run("check"));
    let second = fs::metadata(target).unwrap();
    assert_eq!(
        (first.ino(), first.mtime_nsec()),
        (second.ino(), second.mtime_nsec())
    );
    assert_eq!(
        state.ino(),
        fs::metadata(f.path("state/fixture.json")).unwrap().ino()
    );
    assert_eq!(first.mode() & 0o777, 0o640);
}

#[test]
fn renders_strict_templates_and_declared_includes() {
    let mut f =
        Fixture::new(json!([{"kind":"file","path":"/example","source":"source","template":true}]));
    fs::write(f.path("source"), "{% include 'header' %}hello {{ name }}\n").unwrap();
    fs::write(f.path("header"), "# rendered\n").unwrap();
    f.manifest["partials"] = json!({"header":"header"});
    success(&f.run("apply"));
    assert_eq!(
        fs::read_to_string(f.path("root/example")).unwrap(),
        "# rendered\nhello world\n"
    );
}

#[test]
fn rendering_error_prevents_all_writes_without_exposing_context() {
    let mut f = Fixture::new(
        json!([file("/first"),{"kind":"file","path":"/second","source":"bad","template":true,"mode":"0600","sensitive":true}]),
    );
    f.manifest["context"]["secret"] = json!("TEST-SECRET-DO-NOT-LOG");
    fs::write(f.path("bad"), "{{ secret }} {{ missing }}").unwrap();
    let output = f.run("apply");
    assert_eq!(output.status.code(), Some(2));
    assert!(!f.path("root/first").exists());
    assert!(!f.path("state").exists());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("TEST-SECRET"));
}

#[test]
fn unmanaged_file_and_local_edits_are_conflicts_for_whole_batch() {
    let mut f = Fixture::new(json!([file("/first"), file("/second")]));
    fs::write(f.path("root/second"), "local").unwrap();
    assert_eq!(f.run("apply").status.code(), Some(2));
    assert!(!f.path("root/first").exists());
    fs::remove_file(f.path("root/second")).unwrap();
    success(&f.run("apply"));
    fs::write(f.path("root/second"), "local edit").unwrap();
    fs::write(f.path("source"), "new source").unwrap();
    assert_eq!(f.run("apply").status.code(), Some(2));
    assert_eq!(fs::read_to_string(f.path("root/first")).unwrap(), "hello\n");
    f.manifest["entries"][1]["create_only"] = json!(true);
    success(&f.run("apply"));
    assert_eq!(
        fs::read_to_string(f.path("root/second")).unwrap(),
        "local edit"
    );
}

#[test]
fn source_update_and_metadata_update_backup_and_restore() {
    let f = Fixture::new(json!([file("/example")]));
    success(&f.run("apply"));
    fs::write(f.path("source"), "updated\n").unwrap();
    success(&f.run("apply"));
    let receipt = fs::read_dir(f.path("state"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .find(|p| p.join("0.bin").exists())
        .unwrap()
        .join("receipt.json");
    assert_eq!(
        fs::read_to_string(receipt.parent().unwrap().join("0.bin")).unwrap(),
        "hello\n"
    );
    success(&f.restore(&receipt));
    assert_eq!(
        fs::read_to_string(f.path("root/example")).unwrap(),
        "hello\n"
    );
    assert_eq!(
        fs::metadata(f.path("root/example")).unwrap().mode() & 0o777,
        0o640
    );
}

#[test]
fn restore_refuses_later_local_edits() {
    let f = Fixture::new(json!([file("/example")]));
    success(&f.run("apply"));
    fs::write(f.path("root/example"), "local edit").unwrap();
    assert_eq!(f.restore(&f.receipt()).status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(f.path("root/example")).unwrap(),
        "local edit"
    );
}

#[test]
fn symlinks_are_managed_without_following_destination() {
    let mut f = Fixture::new(json!([{"kind":"symlink","path":"/link","target":"/missing"}]));
    success(&f.run("apply"));
    success(&f.run("check"));
    f.manifest["entries"][0]["target"] = json!("relative");
    success(&f.run("apply"));
    assert_eq!(
        fs::read_link(f.path("root/link")).unwrap(),
        Path::new("relative")
    );
}

#[test]
fn parent_symlinks_and_traversal_cannot_escape_root() {
    let f = Fixture::new(json!([file("/escape/file")]));
    fs::create_dir(f.path("outside")).unwrap();
    symlink(f.path("outside"), f.path("root/escape")).unwrap();
    assert_eq!(f.run("apply").status.code(), Some(2));
    assert!(!f.path("outside/file").exists());
    for path in ["/../escape", "relative", "/", "/first/../second"] {
        let f = Fixture::new(json!([file(path)]));
        assert_eq!(f.run("apply").status.code(), Some(2));
    }
}

#[test]
fn duplicate_overlapping_paths_and_source_escape_rejected() {
    for entries in [
        json!([file("/same"), file("/same")]),
        json!([file("/parent"), file("/parent/child")]),
        json!([{"kind":"file","path":"/x","source":"../outside"}]),
    ] {
        let f = Fixture::new(entries);
        assert_eq!(f.run("apply").status.code(), Some(2));
        assert!(!f.path("state").exists());
    }
}

#[test]
fn conditions_host_guard_and_create_only() {
    let mut f = Fixture::new(json!([{"kind":"file","path":"/x","source":"source","when":"false"}]));
    success(&f.run("apply"));
    assert!(!f.path("root/x").exists());
    f.manifest["entries"][0]["when"] = json!("missing");
    assert_eq!(f.run("apply").status.code(), Some(2));
    f.manifest["entries"][0]["when"] = json!("true");
    f.manifest["host"] = json!("not-the-current-host");
    assert_eq!(f.run("apply").status.code(), Some(2));
}

#[test]
fn private_modes_unknown_fields_and_invalid_data_fail_closed() {
    for entry in [
        json!({"kind":"file","path":"/x","source":"source","sensitive":true}),
        json!({"kind":"file","path":"/x","source":"source","mode":"4755"}),
        json!({"kind":"file","path":"/x","source":"source","typo":true}),
        json!({"kind":"symlink","path":"/x","target":"y","mode":"0644"}),
    ] {
        let f = Fixture::new(json!([entry]));
        assert_eq!(f.run("apply").status.code(), Some(2));
    }
}

#[test]
fn deletion_requires_owned_baseline_and_restores_file() {
    let mut f = Fixture::new(json!([file("/example")]));
    success(&f.run("apply"));
    f.manifest["entries"] = json!([{"kind":"absent","path":"/example"}]);
    success(&f.run("apply"));
    assert!(!f.path("root/example").exists());
    let receipt = fs::read_dir(f.path("state"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.join("0.bin").exists())
        .unwrap()
        .join("receipt.json");
    success(&f.restore(&receipt));
    assert_eq!(
        fs::read_to_string(f.path("root/example")).unwrap(),
        "hello\n"
    );
}

#[test]
fn metadata_drift_conflicts_and_manifest_mode_changes_apply() {
    let mut f = Fixture::new(json!([file("/example")]));
    success(&f.run("apply"));
    f.manifest["entries"][0]["mode"] = json!("0600");
    success(&f.run("apply"));
    fs::set_permissions(f.path("root/example"), fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(f.run("check").status.code(), Some(2));
}

#[test]
fn plan_is_read_only_and_does_not_print_sensitive_content() {
    let f = Fixture::new(
        json!([{"kind":"file","path":"/secret","source":"source","mode":"0600","sensitive":true}]),
    );
    fs::write(f.path("source"), "TEST-SECRET").unwrap();
    let out = f.run("plan");
    assert_eq!(out.status.code(), Some(1));
    assert!(!f.path("state").exists());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("TEST-SECRET"));
}

#[test]
fn unsafe_state_path_and_state_root_mismatch_rejected() {
    let f = Fixture::new(json!([file("/example")]));
    fs::create_dir(f.path("outside")).unwrap();
    symlink(f.path("outside"), f.path("state")).unwrap();
    assert_eq!(f.run("apply").status.code(), Some(2));
    fs::remove_file(f.path("state")).unwrap();
    success(&f.run("apply"));
    let p = f.path("state/fixture.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
    state["root"] = json!("/");
    fs::write(p, serde_json::to_vec(&state).unwrap()).unwrap();
    assert_eq!(f.run("check").status.code(), Some(2));
}

#[test]
fn interrupted_success_is_recovered_and_checkpoint_is_cleared() {
    let f = Fixture::new(json!([file("/example")]));
    success(&f.run("apply"));
    let path = f.path("state/fixture.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let after = state["files"]["/example"].clone();
    state["files"] = json!({});
    state["pending"] = json!({"path":"/example","before":{"kind":"absent"},"after":after});
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    success(&f.run("check"));
    assert_eq!(before, fs::read(&path).unwrap());
    success(&f.run("apply"));
    let state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(state["pending"].is_null());
    assert_eq!(state["files"]["/example"], after);
}

#[test]
fn interrupted_apply_refuses_unexpected_content() {
    let f = Fixture::new(json!([file("/example")]));
    success(&f.run("apply"));
    let path = f.path("state/fixture.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state["pending"] =
        json!({"path":"/example","before":{"kind":"absent"},"after":state["files"]["/example"]});
    fs::write(path, serde_json::to_vec(&state).unwrap()).unwrap();
    fs::write(f.path("root/example"), "unexpected edit").unwrap();
    assert_eq!(f.run("apply").status.code(), Some(2));
}

#[test]
fn corrupted_backup_cannot_be_restored() {
    let f = Fixture::new(json!([file("/example")]));
    success(&f.run("apply"));
    fs::write(f.path("source"), "new content").unwrap();
    success(&f.run("apply"));
    let folder = fs::read_dir(f.path("state"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.join("0.bin").exists())
        .unwrap();
    fs::write(folder.join("0.bin"), "corrupt").unwrap();
    assert_eq!(
        f.restore(&folder.join("receipt.json")).status.code(),
        Some(2)
    );
    assert_eq!(
        fs::read_to_string(f.path("root/example")).unwrap(),
        "new content"
    );
}

#[test]
fn extended_attributes_survive_replacement_and_backup() {
    use std::os::unix::ffi::OsStrExt;
    let f = Fixture::new(json!([file("/example")]));
    success(&f.run("apply"));
    let path = std::ffi::CString::new(f.path("root/example").as_os_str().as_bytes()).unwrap();
    let value = b"test-attribute";
    assert_eq!(
        unsafe {
            libc::setxattr(
                path.as_ptr(),
                c"user.tjikup-test".as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        },
        0
    );
    fs::write(f.path("source"), "changed").unwrap();
    success(&f.run("apply"));
    let mut result = vec![0u8; 64];
    let count = unsafe {
        libc::getxattr(
            path.as_ptr(),
            c"user.tjikup-test".as_ptr(),
            result.as_mut_ptr().cast(),
            result.len(),
        )
    };
    assert!(count >= 0);
    result.truncate(count as usize);
    assert_eq!(result, value);
    let folder = fs::read_dir(f.path("state"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.join("0.bin").exists())
        .unwrap();
    let receipt: Value =
        serde_json::from_slice(&fs::read(folder.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(
        receipt["changes"][0]["xattrs"]["user.tjikup-test"],
        json!(value)
    );
    success(&f.restore(&folder.join("receipt.json")));
}

#[test]
fn existing_recipe_checks_drive_native_apply_then_skip() {
    let f = Fixture::new(json!([file("/example")]));
    // Write the native manifest without applying it.
    assert_eq!(f.run("check").status.code(), Some(1));
    let binary = env!("CARGO_BIN_EXE_tjikup");
    let args = |operation: &str| {
        json!([
            binary,
            "files",
            operation,
            "--manifest",
            f.path("manifest.json"),
            "--root",
            f.path("root"),
            "--state-dir",
            f.path("state")
        ])
    };
    let recipe = json!({"version":2,"recipes":[{"id":"native","check":args("check"),"apply":args("apply"),"resources":["files"]}]});
    fs::write(f.path("recipes.json"), serde_json::to_vec(&recipe).unwrap()).unwrap();
    let run = || {
        Command::new(binary)
            .arg("--config")
            .arg(f.path("recipes.json"))
            .env("XDG_STATE_HOME", f.path("runner-state"))
            .env("TJIKUP_NOTIFY", "0")
            .output()
            .unwrap()
    };
    success(&run());
    let first = fs::metadata(f.path("root/example")).unwrap();
    success(&run());
    let second = fs::metadata(f.path("root/example")).unwrap();
    assert_eq!(
        (first.ino(), first.mtime_nsec()),
        (second.ino(), second.mtime_nsec())
    );
}

#[test]
fn concurrent_apply_lock_blocks_without_touching_destinations() {
    use std::os::fd::AsRawFd;
    let f = Fixture::new(json!([file("/example")]));
    success(&f.run("apply"));
    fs::write(f.path("source"), "new content").unwrap();
    let lock = fs::File::open(f.path("state/lock")).unwrap();
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    assert_eq!(f.run("apply").status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(f.path("root/example")).unwrap(),
        "hello\n"
    );
}

#[test]
fn private_directories_order_children_and_restore_without_recursive_deletion() {
    let f = Fixture::new(json!([
        {"kind":"file","path":"/private/nested/config","source":"source"},
        {"kind":"directory","path":"/private","mode":"0700"},
        {"kind":"directory","path":"/private/nested","mode":"0700"}
    ]));
    success(&f.run("apply"));
    assert_eq!(
        fs::metadata(f.path("root/private")).unwrap().mode() & 0o777,
        0o700
    );
    let before = fs::metadata(f.path("root/private")).unwrap();
    success(&f.run("apply"));
    assert_eq!(
        fs::metadata(f.path("root/private")).unwrap().ino(),
        before.ino()
    );
    let receipt = f.receipt();
    fs::write(f.path("root/private/unmanaged"), "preserve").unwrap();
    assert_eq!(f.restore(&receipt).status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(f.path("root/private/unmanaged")).unwrap(),
        "preserve"
    );
    fs::remove_file(f.path("root/private/unmanaged")).unwrap();
    success(&f.restore(&receipt));
    assert!(!f.path("root/private").exists());
}

#[test]
fn directory_mode_changes_restore_and_never_replace_files() {
    let mut f = Fixture::new(json!([{"kind":"directory","path":"/private","mode":"0700"}]));
    success(&f.run("apply"));
    f.manifest["entries"][0]["mode"] = json!("0750");
    let out = f.run("apply");
    success(&out);
    let text = String::from_utf8(out.stdout).unwrap();
    let receipt = PathBuf::from(
        text.lines()
            .find_map(|s| s.strip_prefix("Receipt: "))
            .unwrap(),
    );
    success(&f.restore(&receipt));
    assert_eq!(
        fs::metadata(f.path("root/private")).unwrap().mode() & 0o777,
        0o700
    );
    f.manifest["entries"] = json!([{"kind":"file","path":"/private","source":"source"}]);
    assert_eq!(f.run("apply").status.code(), Some(2));
    assert!(f.path("root/private").is_dir());
}

#[test]
fn shared_entry_and_context_files_reload_and_reject_traversal() {
    let mut f = Fixture::new(json!([]));
    f.manifest["context_files"] = json!(["data.json"]);
    f.manifest["entries_files"] = json!(["entries.json"]);
    fs::write(f.path("data.json"), r#"{"value":"captured"}"#).unwrap();
    fs::write(
        f.path("entries.json"),
        r#"[{"kind":"file","path":"/config","source":"source","template":true}]"#,
    )
    .unwrap();
    fs::write(f.path("source"), "{{ value }} {{ name }}").unwrap();
    success(&f.run("apply"));
    assert_eq!(
        fs::read_to_string(f.path("root/config")).unwrap(),
        "captured world"
    );
    fs::write(f.path("data.json"), r#"{"value":"updated"}"#).unwrap();
    success(&f.run("apply"));
    assert_eq!(
        fs::read_to_string(f.path("root/config")).unwrap(),
        "updated world"
    );
    f.manifest["context_files"] = json!(["../escape.json"]);
    assert_eq!(f.run("apply").status.code(), Some(2));
}

#[test]
fn grouped_apply_preserves_other_groups_state_and_drift_checks() {
    let f = Fixture::new(json!([
        {"kind":"file","path":"/a","source":"source","group":"a"},
        {"kind":"file","path":"/b","source":"source","group":"b"}
    ]));
    success(&f.run_args("apply", &["--group", "a"]));
    assert!(!f.path("root/b").exists());
    success(&f.run_args("apply", &["--group", "b"]));
    let state: Value =
        serde_json::from_slice(&fs::read(f.path("state/fixture.json")).unwrap()).unwrap();
    assert_eq!(state["files"].as_object().unwrap().len(), 2);
    fs::write(f.path("root/a"), "local edit").unwrap();
    success(&f.run_args("check", &["--group", "b"]));
    assert!(!f.run_args("apply", &["--group", "a"]).status.success());
    assert_eq!(fs::read_to_string(f.path("root/a")).unwrap(), "local edit");
    assert!(!f.run_args("plan", &["--group", "typo"]).status.success());
}
#[test]
fn group_selection_excludes_other_templates_before_rendering_or_secret_resolution() {
    let f = Fixture::new(json!([
        {"kind":"file","path":"/a","source":"source","group":"a"},
        {"kind":"file","path":"/secret","source":"secret","template":true,"group":"secret"}
    ]));
    fs::write(
        f.path("secret"),
        "{{ secret('11111111-2222-3333-4444-555555555555') }}",
    )
    .unwrap();
    success(&f.run_args("apply", &["--group", "a"]));
    assert!(!f.path("root/secret").exists());
    assert!(!f.run("plan").status.success());
    assert!(!f.run_args("restore", &["--group", "a"]).status.success());
}

#[test]
fn identical_adoption_changes_only_metadata_and_refuses_later_drift() {
    let f = Fixture::new(json!([{"kind":"file","path":"/owned","source":"source","mode":"0600"}]));
    fs::write(f.path("root/owned"), "hello\n").unwrap();
    fs::set_permissions(f.path("root/owned"), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!f.run("apply").status.success());
    success(&f.run_args("apply", &["--adopt-identical"]));
    assert_eq!(
        fs::metadata(f.path("root/owned")).unwrap().mode() & 0o777,
        0o600
    );
    fs::write(f.path("root/owned"), "local edits\n").unwrap();
    assert!(!f.run_args("apply", &["--adopt-identical"]).status.success());
    assert_eq!(
        fs::read_to_string(f.path("root/owned")).unwrap(),
        "local edits\n"
    );
}

#[test]
fn adoption_refuses_symlinks_and_preflights_all_files() {
    let f = Fixture::new(json!([
        {"kind":"file","path":"/first","source":"source","mode":"0600"},
        {"kind":"file","path":"/second","source":"source","mode":"0600"}
    ]));
    fs::write(f.path("root/first"), "hello\n").unwrap();
    fs::set_permissions(f.path("root/first"), fs::Permissions::from_mode(0o644)).unwrap();
    symlink(f.path("source"), f.path("root/second")).unwrap();
    assert!(!f.run_args("apply", &["--adopt-identical"]).status.success());
    assert_eq!(
        fs::metadata(f.path("root/first")).unwrap().mode() & 0o777,
        0o644
    );
    assert!(!f.path("state/fixture.json").exists());
}
