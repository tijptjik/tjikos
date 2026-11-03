use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

const APP: &str = "org.example.TjikupFixture";
const BUNDLE: &str = "synthetic-flatpak-bundle\n";

fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn executable(path: &Path, text: &str) {
    write(path, text);
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn fixture(installation: &str, current: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("home")).unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    write(
        &root.join("native/installers.json"),
        json!({
            "fixture": {
                "repository": "owner/tool", "latest": true,
                "kind": "flatpak-bundle", "asset": "tool-{version}-{arch}.flatpak",
                "flatpak": { "app_id": APP, "branch": "stable", "installation": installation }
            }
        })
        .to_string(),
    );
    let asset = format!("tool-1.2.3-{}.flatpak", std::env::consts::ARCH);
    write(&root.join("release.json"), json!({
        "id": 1, "tag_name": "v1.2.3", "draft": false, "prerelease": false,
        "assets": [{ "id": 2, "name": asset,
            "browser_download_url": format!("https://github.com/owner/tool/releases/download/v1.2.3/{asset}"),
            "digest": format!("sha256:{:x}", Sha256::digest(BUNDLE)), "size": BUNDLE.len()
        }]
    }).to_string());
    if let Some(version) = current {
        write(&root.join("current-version"), version);
        write(&root.join("current-commit"), "a".repeat(64));
    }
    write(&root.join("home/profile"), "private app preferences\n");
    executable(
        &root.join("bin/curl"),
        r##"#!/bin/sh
headers=
download=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --dump-header) shift; headers=$1 ;;
        --output) shift; download=$1 ;;
    esac
    shift
done
if [ -n "$download" ]; then
    printf 'download\n' >> "$MOCK_ROOT/events"
    if [ "$MOCK_MODE" = corrupt ]; then printf bad > "$download"; else printf 'synthetic-flatpak-bundle\n' > "$download"; fi
else
    if [ -n "$headers" ]; then printf 'HTTP/1.1 200 OK\r\nETag: "fixture"\r\n\r\n' > "$headers"; fi
    /usr/bin/cat "$MOCK_ROOT/release.json"
fi
"##,
    );
    executable(
        &root.join("bin/sudo"),
        r##"#!/bin/sh
printf 'sudo\n' >> "$MOCK_ROOT/events"
[ "$1" = -n ] && shift
[ "$1" = -- ] && shift
exec "$@"
"##,
    );
    executable(
        &root.join("bin/flatpak"),
        r##"#!/bin/sh
reference="app/org.example.TjikupFixture/$MOCK_ARCH/stable"
commit=cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
staged=no
if [ -n "$FLATPAK_USER_DIR" ]; then
    staged=yes
    case "$XDG_CONFIG_HOME:$XDG_DATA_HOME:$XDG_CACHE_HOME:$XDG_STATE_HOME" in
        "$FLATPAK_USER_DIR"*) exit 90 ;;
    esac
    [ -n "$XDG_CONFIG_HOME" ] && [ -n "$XDG_DATA_HOME" ] && [ -n "$XDG_CACHE_HOME" ] && [ -n "$XDG_STATE_HOME" ] || exit 91
fi
case "$1" in
install)
    if [ "$staged" = yes ]; then
        case " $* " in *' --user '*) ;; *) exit 92 ;; esac
        for required in --bundle --no-deps --no-related --no-pull --noninteractive; do
            case " $* " in *" $required "*) ;; *) exit 93 ;; esac
        done
        printf 'stage\n' >> "$MOCK_ROOT/events"
        if [ "$MOCK_MODE" = malformed_bundle ]; then exit 19; fi
        if [ "$MOCK_MODE" = newer_during_staging ]; then
            printf '2.0.0' > "$MOCK_ROOT/current-version"
            printf '%s' "$commit" > "$MOCK_ROOT/current-commit"
        fi
        if [ "$MOCK_MODE" = uninstall_during_staging ]; then
            /usr/bin/rm -f "$MOCK_ROOT/current-version" "$MOCK_ROOT/current-commit"
        fi
        if [ "$MOCK_MODE" = install_during_staging ]; then
            printf '1.0.0' > "$MOCK_ROOT/current-version"
            printf '%s' "$commit" > "$MOCK_ROOT/current-commit"
        fi
        exit 0
    fi
    case " $* " in *" --$MOCK_INSTALLATION "*) ;; *) exit 94 ;; esac
    case " $* " in *' --or-update '*) ;; *) exit 95 ;; esac
    printf 'apply\n' >> "$MOCK_ROOT/events"
    if [ "$MOCK_MODE" = install_failure ]; then exit 17; fi
    printf '1.2.3' > "$MOCK_ROOT/current-version"
    if [ "$MOCK_MODE" = bad_commit ]; then commit=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb; fi
    printf '%s' "$commit" > "$MOCK_ROOT/current-commit"
    ;;
list)
    if [ "$staged" = no ]; then printf 'org.example.Unversioned/%s/stable\n' "$MOCK_ARCH"; fi
    if [ "$staged" = yes ]; then
        version=1.2.3
        case "$MOCK_MODE" in
            wrong_id) reference="app/org.example.Other/$MOCK_ARCH/stable" ;;
            wrong_arch) reference="app/org.example.TjikupFixture/unsupported/stable" ;;
            wrong_branch) reference="app/org.example.TjikupFixture/$MOCK_ARCH/beta" ;;
            wrong_version) version=9.8.7 ;;
        esac
        printf '%s\t%s\n' "$reference" "$version"
    elif [ -f "$MOCK_ROOT/current-version" ]; then
        version=$(/usr/bin/cat "$MOCK_ROOT/current-version")
        printf '%s\t%s\n' "$reference" "$version"
    fi
    ;;
info)
    if [ "$staged" = no ]; then
        commit=$(/usr/bin/cat "$MOCK_ROOT/current-commit")
        version=$(/usr/bin/cat "$MOCK_ROOT/current-version")
        if [ "$MOCK_MODE" = wrong_deployed_ref ] && [ "$version" = 1.2.3 ]; then reference="app/org.example.Other/$MOCK_ARCH/stable"; fi
    fi
    printf '%s %s\n' "$reference" "$commit"
    ;;
*) exit 96 ;;
esac
"##,
    );
    dir
}

fn run(root: &Path, installation: &str, mode: &str) -> Output {
    run_operation(root, installation, mode, "")
}

fn run_operation(root: &Path, installation: &str, mode: &str, operation: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tjikup"))
        .args(["workflow", "--repo"])
        .arg(root)
        .args(["install", "fixture"])
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", root.join("bin"))
        .env("MOCK_ROOT", root)
        .env("MOCK_MODE", mode)
        .env("MOCK_INSTALLATION", installation)
        .env("MOCK_ARCH", std::env::consts::ARCH)
        .env_remove("FLATPAK_USER_DIR")
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env("TJIKUP_OPERATION", operation)
        .output()
        .unwrap()
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn events(root: &Path) -> String {
    fs::read_to_string(root.join("events")).unwrap_or_default()
}

#[test]
fn flatpak_release_updates_only_declared_scope_and_preserves_user_data() {
    for installation in ["system", "user"] {
        let fixture = fixture(installation, Some("1.0.0"));
        let root = fixture.path();
        let output = run(root, installation, "");
        success(&output);
        assert!(String::from_utf8_lossy(&output.stdout).contains("changed"));
        assert_eq!(
            events(root),
            format!(
                "download\nstage\n{}apply\n",
                if installation == "system" {
                    "sudo\n"
                } else {
                    ""
                }
            )
        );
        assert_eq!(
            fs::read_to_string(root.join("current-version")).unwrap(),
            "1.2.3"
        );
        assert_eq!(
            fs::read_to_string(root.join("home/profile")).unwrap(),
            "private app preferences\n"
        );
        write(&root.join("events"), "");
        success(&run(root, installation, ""));
        assert_eq!(events(root), "");
    }
}

#[test]
fn flatpak_release_rejects_wrong_bundle_identity_before_live_mutation() {
    for mode in [
        "wrong_id",
        "wrong_arch",
        "wrong_branch",
        "wrong_version",
        "corrupt",
        "malformed_bundle",
    ] {
        let fixture = fixture("system", Some("1.0.0"));
        let root = fixture.path();
        let output = run(root, "system", mode);
        assert!(!output.status.success(), "accepted {mode}");
        assert!(!events(root).contains("apply"));
        assert!(!events(root).contains("sudo"));
        assert_eq!(
            fs::read_to_string(root.join("current-version")).unwrap(),
            "1.0.0"
        );
    }
}

#[test]
fn flatpak_release_rechecks_installed_version_after_bundle_staging() {
    let fixture = fixture("system", Some("1.0.0"));
    let root = fixture.path();
    let output = run(root, "system", "newer_during_staging");
    success(&output);
    assert_eq!(events(root), "download\nstage\n");
    assert_eq!(
        fs::read_to_string(root.join("current-version")).unwrap(),
        "2.0.0"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("TJIKUP_RESULT=changed"));
}

#[test]
fn flatpak_release_preserves_operation_scope_when_presence_changes_during_staging() {
    let fixture = fixture("system", Some("1.0.0"));
    let root = fixture.path();
    let output = run_operation(root, "system", "uninstall_during_staging", "update");
    success(&output);
    assert_eq!(events(root), "download\nstage\n");
    assert!(!root.join("current-version").exists());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("TJIKUP_RESULT=changed"));

    let fixture = self::fixture("system", None);
    let root = fixture.path();
    let output = run_operation(root, "system", "install_during_staging", "install");
    success(&output);
    assert_eq!(events(root), "download\nstage\n");
    assert_eq!(
        fs::read_to_string(root.join("current-version")).unwrap(),
        "1.0.0"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("TJIKUP_RESULT=changed"));
}

#[test]
fn flatpak_release_never_downgrades_or_guesses_unknown_installed_version() {
    for current in ["1.2.3", "2.0.0", "unknown"] {
        let fixture = fixture("system", Some(current));
        let root = fixture.path();
        let output = run(root, "system", "");
        if current == "unknown" {
            assert!(!output.status.success());
        } else {
            success(&output);
        }
        assert_eq!(events(root), "");
        assert_eq!(
            fs::read_to_string(root.join("current-version")).unwrap(),
            current
        );
    }
}

#[test]
fn flatpak_release_requires_successful_post_install_verification() {
    for mode in ["bad_commit", "install_failure", "wrong_deployed_ref"] {
        let fixture = fixture("system", Some("1.0.0"));
        let output = run(fixture.path(), "system", mode);
        assert!(!output.status.success(), "accepted {mode}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("TJIKUP_RESULT=changed"));
        let receipts = fixture.path().join("state/tjikup/installers/flatpak");
        if let Ok(entries) = fs::read_dir(receipts) {
            assert!(!entries
                .filter_map(Result::ok)
                .any(|entry| entry.path().extension().is_some_and(|ext| ext == "json")));
        }
    }
}
