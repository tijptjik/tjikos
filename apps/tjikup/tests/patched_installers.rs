//! Source upgrades use real local git-apply with synthetic metadata/tools; no remotes or privileges.
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
fn patch(before: &str, after: &str) -> String {
    format!("diff --git a/source.txt b/source.txt\n--- a/source.txt\n+++ b/source.txt\n@@ -1 +1 @@\n-{before}\n+{after}\n")
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for name in ["native", "bin", "home", "upstream"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    fs::write(root.join("upstream/source.txt"), "before\n").unwrap();
    fs::write(root.join("fix.patch"), patch("before", "after")).unwrap();
    fs::write(root.join("native/installers.json"), json!({"fixture":{
        "repository":"owner/fixture", "kind":"git-build", "latest":true,
        "binary":"bin/fixture", "prefix":"{home}/versions/{patch_digest}/{version}",
        "links":{"{home}/.local/bin/fixture":"{prefix}/bin/fixture"},
        "patches":["{repo}/fix.patch"], "file_capability":"cap_dac_override=p",
        "steps":[["sh","-c","grep -q '^after' source.txt && echo build >> \"$MOCK_ROOT/events\" && mkdir -p \"$1/bin\" && printf '#!/bin/sh\\necho fixture\\n' > \"$1/bin/fixture\" && chmod +x \"$1/bin/fixture\"","_","{stage}"]]
    }}).to_string()).unwrap();
    executable(
        &root.join("bin/curl"),
        r##"#!/bin/sh
previous=
for arg in "$@"; do
 if [ "$previous" = --dump-header ]; then printf 'HTTP/1.1 200 OK\r\n\r\n' > "$arg"; fi
 previous=$arg
done
for arg in "$@"; do
 case "$arg" in
  *'/commits/'*) printf '{"sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}'; exit 0 ;;
 esac
done
printf '{"tag_name":"v1.2.3","draft":false,"prerelease":false,"assets":[]}'
"##,
    );
    executable(&root.join("bin/gh"), "#!/bin/sh\nexit 1\n");
    executable(
        &root.join("bin/git"),
        r##"#!/bin/sh
if [ "$1" = clone ]; then
 for last in "$@"; do :; done
 mkdir -p "$last"
 cp "$MOCK_ROOT/upstream/source.txt" "$last/"
 /usr/bin/git -C "$last" init -q
 exit $?
fi
if [ "$3" = rev-parse ]; then printf 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n'; exit 0; fi
exec /usr/bin/git "$@"
"##,
    );
    executable(
        &root.join("bin/getcap"),
        r##"#!/bin/sh
if [ -f "$MOCK_ROOT/capability" ]; then
 line=$(cat "$MOCK_ROOT/capability")
 case "$line" in "$1 "*) printf '%s\n' "$line" ;; esac
fi
"##,
    );
    executable(
        &root.join("bin/sudo"),
        r##"#!/bin/sh
[ "$1" = -n ] && shift
[ "$1" = -- ] && shift
[ "$1" = setcap ] || exit 97
[ ! -f "$MOCK_ROOT/fail-capability" ] || exit 98
printf '%s %s\n' "$3" "$2" > "$MOCK_ROOT/capability"
echo capability >> "$MOCK_ROOT/events"
"##,
    );
    dir
}
fn run(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tjikup"))
        .args(["workflow", "--repo"])
        .arg(root)
        .args(["install", "fixture"])
        .env("HOME", root.join("home"))
        .env("XDG_CACHE_HOME", root.join("home/cache"))
        .env("XDG_STATE_HOME", root.join("home/state"))
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", root.join("bin").display()),
        )
        .env("MOCK_ROOT", root)
        .env("TJIKUP_OPERATION", "")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN")
        .output()
        .unwrap()
}
fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn builds(root: &Path) -> usize {
    fs::read_to_string(root.join("events"))
        .unwrap()
        .lines()
        .filter(|line| *line == "build")
        .count()
}
#[test]
fn patch_identity_controls_rebuild_and_capability_is_repaired_before_activation() {
    let dir = fixture();
    let root = dir.path();
    success(run(root));
    let link = root.join("home/.local/bin/fixture");
    let original = fs::read_link(&link).unwrap();
    assert_eq!(builds(root), 1);
    success(run(root));
    assert_eq!(builds(root), 1);
    fs::remove_file(root.join("capability")).unwrap();
    success(run(root));
    assert_eq!(builds(root), 1);
    assert!(fs::read_to_string(root.join("capability"))
        .unwrap()
        .contains("cap_dac_override=p"));
    fs::write(root.join("fix.patch"), patch("before", "after-two")).unwrap();
    fs::write(root.join("fail-capability"), "").unwrap();
    assert!(!run(root).status.success());
    assert_eq!(fs::read_link(&link).unwrap(), original);
    fs::remove_file(root.join("fail-capability")).unwrap();
    success(run(root));
    assert_ne!(fs::read_link(&link).unwrap(), original);
    assert_eq!(builds(root), 2);
}
#[test]
fn incompatible_upgrade_preserves_live_link_and_already_applied_patch_is_accepted() {
    let dir = fixture();
    let root = dir.path();
    success(run(root));
    let link = root.join("home/.local/bin/fixture");
    let original = fs::read_link(&link).unwrap();
    fs::write(
        root.join("fix.patch"),
        patch("different-upstream", "after-three"),
    )
    .unwrap();
    let output = run(root);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("installed version preserved"));
    assert_eq!(fs::read_link(&link).unwrap(), original);
    assert_eq!(builds(root), 1);
    fs::write(root.join("upstream/source.txt"), "after-three\n").unwrap();
    success(run(root));
    assert_ne!(fs::read_link(&link).unwrap(), original);
    assert_eq!(builds(root), 2);
}
