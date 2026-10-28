use serde_json::{json, Value};
use std::cell::Cell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn commit(path: &Path, name: &str) {
    fs::write(path.join(name), name).unwrap();
    git(path, &["add", name]);
    git(
        path,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            name,
        ],
    );
}
struct Fixture {
    dir: TempDir,
    local: PathBuf,
    peer: PathBuf,
    origin: PathBuf,
    run: Cell<usize>,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        let local = dir.path().join("local");
        let peer = dir.path().join("peer");
        git(
            dir.path(),
            &[
                "init",
                "--bare",
                "--initial-branch=main",
                origin.to_str().unwrap(),
            ],
        );
        git(
            dir.path(),
            &[
                "clone",
                "-q",
                origin.to_str().unwrap(),
                local.to_str().unwrap(),
            ],
        );
        git(&local, &["config", "user.name", "Fixture"]);
        git(&local, &["config", "user.email", "fixture@example.invalid"]);
        git(&local, &["config", "commit.gpgsign", "false"]);
        commit(&local, "initial");
        git(&local, &["push", "-qu", "origin", "main"]);
        git(
            dir.path(),
            &[
                "clone",
                "-q",
                origin.to_str().unwrap(),
                peer.to_str().unwrap(),
            ],
        );
        Self {
            dir,
            local,
            peer,
            origin,
            run: Cell::new(0),
        }
    }
    fn upstream(&self, count: usize) {
        for i in 0..count {
            commit(&self.peer, &format!("upstream-{i}"));
        }
        git(&self.peer, &["push", "-q"]);
    }
    fn invoke(&self, pull: &str, skip: bool) -> (bool, String, Value) {
        let config = json!({"version":2,"env":{"SKIP":"0"},"recipes":[
            {"id":"pull","git":{"repository":self.local,"operation":pull},"when":{"environment":"SKIP","equals":if skip {"1"} else {"0"}},"display":{"repository":"dotjik","operation":"pull"}},
            {"id":"push","needs":["pull"],"git":{"repository":self.local,"operation":"push"},"when":{"environment":"SKIP","equals":if skip {"1"} else {"0"}},"display":{"repository":"dotjik","operation":"push"}}
        ]});
        let config_path = self.dir.path().join("config.json");
        fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let state = self.dir.path().join(format!("state-{}", self.run.get()));
        self.run.set(self.run.get() + 1);
        let output = Command::new(env!("CARGO_BIN_EXE_tjikup"))
            .arg("--config")
            .arg(config_path)
            .env("XDG_STATE_HOME", &state)
            .env("TJIKUP_NOTIFY", "0")
            .env("TJIKUP_COLOR", "0")
            .env("TERM", "dumb")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        let run = fs::read_dir(state.join("tjikup/logs"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let receipt = serde_json::from_slice(&fs::read(run.join("receipt.json")).unwrap()).unwrap();
        (
            output.status.success(),
            String::from_utf8(output.stderr).unwrap(),
            receipt,
        )
    }
}
#[test]
fn fast_forward_counts_and_second_run_zeroes_reach_status_and_receipt() {
    let f = Fixture::new();
    f.upstream(3);
    let (ok, text, r) = f.invoke("pull-ff", false);
    assert!(ok, "{text}");
    assert!(text.contains("↓3 ↑0"), "{text}");
    assert_eq!(r["recipes"]["pull"]["git_commits"], 3);
    assert_eq!(r["recipes"]["push"]["git_commits"], 0);
    let (ok, text, r) = f.invoke("pull-ff", false);
    assert!(ok, "{text}");
    assert!(text.contains("↓0 ↑0"), "{text}");
    assert_eq!(r["recipes"]["pull"]["state"], "unchanged");
}
#[test]
fn rebase_counts_only_incoming_commits_and_then_counts_outgoing_commits() {
    let f = Fixture::new();
    commit(&f.local, "local-a");
    commit(&f.local, "local-b");
    f.upstream(3);
    let (ok, text, r) = f.invoke("pull-rebase", false);
    assert!(ok, "{text}");
    assert!(text.contains("↓3 ↑2"), "{text}");
    assert_eq!(r["recipes"]["pull"]["git_commits"], 3);
    assert_eq!(r["recipes"]["push"]["git_commits"], 2);
    assert_eq!(git(&f.origin, &["rev-list", "--count", "main"]), "6");
}
#[test]
fn ahead_only_fast_forward_counts_zero_pulled_and_all_pushed() {
    let f = Fixture::new();
    commit(&f.local, "local-a");
    commit(&f.local, "local-b");
    let (ok, text, r) = f.invoke("pull-ff", false);
    assert!(ok, "{text}");
    assert!(text.contains("↓0 ↑2"), "{text}");
    assert_eq!(r["recipes"]["push"]["git_commits"], 2);
}
#[test]
fn skipped_and_rejected_operations_do_not_claim_zero_or_successful_counts() {
    let f = Fixture::new();
    let (ok, text, r) = f.invoke("pull-ff", true);
    assert!(ok, "{text}");
    assert!(text.contains("↓– ↑–"), "{text}");
    assert!(r["recipes"]["pull"].get("git_commits").is_none());
    commit(&f.local, "local-a");
    let hook = f.origin.join("hooks/pre-receive");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(hook, fs::Permissions::from_mode(0o755)).unwrap();
    let (ok, text, r) = f.invoke("pull-ff", false);
    assert!(!ok);
    assert!(text.contains("↓0 ↑–"), "{text}");
    assert!(!text.contains("↑1"), "{text}");
    assert!(r["recipes"]["push"].get("git_commits").is_none());
    assert_eq!(r["recipes"]["push"]["state"], "failed");
}
#[test]
fn stale_tracking_ref_does_not_overcount_already_published_commits() {
    for unpublished in [false, true] {
        let f = Fixture::new();
        let old = git(&f.local, &["rev-parse", "HEAD"]);
        commit(&f.local, "local-a");
        git(&f.local, &["push", "-q"]);
        git(&f.local, &["update-ref", "refs/remotes/origin/main", &old]);
        if unpublished {
            commit(&f.local, "local-b");
        }
        // Skip pull independently so fetch does not repair the stale ref.
        let config = json!({"version":2,"recipes":[{"id":"push","git":{"repository":f.local,"operation":"push"},"display":{"repository":"dotjik","operation":"push"}}]});
        let path = f.dir.path().join("push.json");
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_tjikup"))
            .arg("--config")
            .arg(path)
            .env("XDG_STATE_HOME", f.dir.path().join("push-state"))
            .env("TJIKUP_NOTIFY", "0")
            .env("TJIKUP_COLOR", "0")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{text}");
        let expected = if unpublished {
            "↓– ↑1"
        } else {
            "↓– ↑0"
        };
        assert!(text.contains(expected), "{text}");
    }
}

#[test]
fn dirty_count_handles_renames_and_newlines_without_inline_logs() {
    let f = Fixture::new();
    git(&f.local, &["mv", "initial", "renamed"]);
    fs::write(f.local.join("new\nfile"), "fixture").unwrap();
    fs::create_dir(f.local.join("nested")).unwrap();
    fs::write(f.local.join("nested/untracked"), "fixture").unwrap();
    let (ok, text, receipt) = f.invoke("pull-ff", false);
    assert!(!ok);
    assert!(text.contains("(3 files)"), "{text}");
    assert!(!text.contains("Log:"), "{text}");
    assert!(!text.contains("commit or stash"), "{text}");
    assert_eq!(receipt["recipes"]["pull"]["dirty_files"], 3);
    assert!(receipt["recipes"]["pull"]["error"]
        .as_str()
        .unwrap()
        .contains("dirty repository"));
}
