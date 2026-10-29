use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::Path,
    process::{Command, Output},
};
fn write(path: &Path, text: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn executable(path: &Path, text: &str) {
    write(path, text);
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    let host = String::from_utf8(Command::new("hostname").output().unwrap().stdout).unwrap();
    write(
        &r.join("device.json"),
        json!({"version":1,"hostname":host.trim(),"role":"laptop","subscriptions":[]}).to_string(),
    );
    write(&r.join("tjikup.json"),json!({"version":2,"device_file":r.join("device.json"),"recipes":[{"id":"base","apply":["true"]}]}).to_string());
    fs::create_dir_all(r.join("home")).unwrap();
    fs::create_dir_all(r.join("bin")).unwrap();
    // SELinux labeling is an OS dependency, independent of fleet interpreters.
    if Path::new("/usr/sbin/matchpathcon").exists() {
        symlink("/usr/sbin/matchpathcon", r.join("bin/matchpathcon")).unwrap();
    }
    dir
}
fn command(root: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_tjikup"));
    c.env("HOME", root.join("home"))
        .env("USER", "fixture")
        .env("DOTJIK_REPO", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", root.join("bin"));
    c
}
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn bootstrap(root: &Path) {
    executable(
        &root.join("bin/provider"),
        "#!/bin/sh\nprintf '{\"id\":\"11111111-2222-3333-4444-555555555555\",\"value\":\"SYNTHETIC-PRIVATE\"}'\n",
    );
    write(
        &root.join("native/home/transport.j2"),
        "token={{ secret('11111111-2222-3333-4444-555555555555') }}\n",
    );
    write(&root.join("native/home/policy.json"), "{\"version\":2}\n");
    write(&root.join("native/home/bootstrap.json"),json!({"version":1,"id":"fixture-bootstrap","bitwarden":{"command":[root.join("bin/provider")]},"entries":[
        {"kind":"file","path":"{{ config_dir }}/synctjik/rclone.conf","source":"transport.j2","template":true,"mode":"0600","sensitive":true,"create_only":true,"when":"not bootstrap_transport_exists"},
        {"kind":"file","path":"{{ config_dir }}/archive-policy.json","source":"policy.json","mode":"0600","create_only":true,"when":"not bootstrap_policy_exists"}
    ]}).to_string());
}
#[test]
fn bootstrap_uses_native_secrets_without_interpreters_and_preserves_existing_files() {
    let d = fixture();
    let r = d.path();
    bootstrap(r);
    let output = command(r)
        .args(["workflow", "bootstrap-archives"])
        .env("BWS_ACCESS_TOKEN", "synthetic")
        .output()
        .unwrap();
    success(&output);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("SYNTHETIC-PRIVATE"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("SYNTHETIC-PRIVATE"));
    let transport = r.join("config/synctjik/rclone.conf");
    assert_eq!(
        fs::read_to_string(&transport).unwrap(),
        "token=SYNTHETIC-PRIVATE\n"
    );
    assert_eq!(
        fs::metadata(&transport).unwrap().permissions().mode() & 0o777,
        0o600
    );
    write(&transport, "local transport\n");
    write(&r.join("config/archive-policy.json"), "local policy\n");
    fs::remove_file(r.join("bin/provider")).unwrap();
    success(
        &command(r)
            .args(["workflow", "bootstrap-archives"])
            .output()
            .unwrap(),
    );
    assert_eq!(fs::read_to_string(transport).unwrap(), "local transport\n");
    assert_eq!(
        fs::read_to_string(r.join("config/archive-policy.json")).unwrap(),
        "local policy\n"
    );
}
#[test]
fn bootstrap_rejects_symlinked_destinations_before_fetching_or_writing() {
    let d = fixture();
    let r = d.path();
    bootstrap(r);
    write(&r.join("other"), "untouched");
    fs::create_dir_all(r.join("config/synctjik")).unwrap();
    symlink(r.join("other"), r.join("config/synctjik/rclone.conf")).unwrap();
    let output = command(r)
        .args(["workflow", "bootstrap-archives"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!r.join("config/archive-policy.json").exists());
    assert_eq!(fs::read_to_string(r.join("other")).unwrap(), "untouched");
}
#[test]
fn source_lookup_supports_portable_enrollment_and_never_fetches_secrets() {
    let d = fixture();
    let r = d.path();
    write(
        &r.join("native/home/settings.j2"),
        "{{ secret('not-fetched') }}\n",
    );
    write(&r.join("native/home/files-device.json"),json!({"version":1,"id":"portable","entries":[{"kind":"file","path":"{{ user.home }}/settings","source":"settings.j2","template":true,"when":"role == 'laptop'"}]}).to_string());
    let output = command(r)
        .args(["config", "home", "source-path", "~/settings"])
        .output()
        .unwrap();
    success(&output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        r.join("native/home/settings.j2").to_str().unwrap()
    );
}
#[test]
fn timing_accept_rejects_invalid_json_and_take_removes_only_valid_owned_report() {
    let d = fixture();
    let r = d.path();
    write(&r.join("report/timings.json.part"), "bad");
    assert!(!command(r)
        .args(["workflow", "timing", "accept"])
        .arg(r.join("report"))
        .status()
        .unwrap()
        .success());
    assert!(!r.join("report/timings.json").exists());
    write(&r.join("report/timings.json.part"), "{\"time\":1}");
    success(
        &command(r)
            .args(["workflow", "timing", "accept"])
            .arg(r.join("report"))
            .output()
            .unwrap(),
    );
    let output = command(r)
        .args(["workflow", "timing", "take"])
        .arg(r.join("report/timings.json"))
        .output()
        .unwrap();
    success(&output);
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({"time":1})
    );
    assert!(!r.join("report/timings.json").exists());
}
#[test]
fn installer_digest_failure_preserves_existing_version_and_profile() {
    let d = fixture();
    let r = d.path();
    let old = r.join("home/old/tool");
    executable(&old, "#!/bin/sh\nexit 0\n");
    fs::create_dir_all(r.join("home/.local/bin")).unwrap();
    symlink(&old, r.join("home/.local/bin/tool")).unwrap();
    write(&r.join("home/profile"), "private profile");
    write(&r.join("native/installers.json"),json!({"fixture":{"repository":"owner/tool","latest":true,"asset":"tool","kind":"raw","binary":"tool","prefix":"{home}/tool/{version}","links":{"{home}/.local/bin/tool":"{prefix}/tool"}}}).to_string());
    let release=json!({"tag_name":"v1.2.3","assets":[{"name":"tool","browser_download_url":"https://github.com/owner/tool/releases/download/v1.2.3/tool","digest":format!("sha256:{}","0".repeat(64))}]}).to_string();
    executable(&r.join("bin/curl"),&format!("#!/bin/sh\nfor arg; do\n if [ \"$save\" = yes ]; then printf bad > \"$arg\"; exit 0; fi\n [ \"$arg\" != --output ] || save=yes\ndone\nprintf '%s' '{}'\n",release));
    let output = command(r)
        .args(["workflow", "install", "fixture"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("checksum"));
    assert_eq!(r.join("home/.local/bin/tool").canonicalize().unwrap(), old);
    assert_eq!(
        fs::read_to_string(r.join("home/profile")).unwrap(),
        "private profile"
    );
}
#[test]
fn native_help_lists_public_commands_without_policy_or_interpreters() {
    let d = fixture();
    let r = d.path();
    for args in [
        ["config", "--help"],
        ["git", "--help"],
        ["ssh", "--help"],
        ["system", "--help"],
        ["workflow", "--help"],
    ] {
        success(&command(r).args(args).output().unwrap());
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    success(&output);
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn git_fixture(root: &Path) {
    for name in ["dotjik", "tjikos"] {
        let remote = root.join(format!("{name}.git"));
        git(
            root,
            &[
                "init",
                "--bare",
                "--initial-branch=main",
                remote.to_str().unwrap(),
            ],
        );
        git(root, &["clone", remote.to_str().unwrap(), name]);
        let repo = root.join(name);
        git(&repo, &["config", "user.email", "fixture@example.invalid"]);
        git(&repo, &["config", "user.name", "Fixture"]);
        write(&repo.join("settings"), "base\n");
        git(&repo, &["add", "settings"]);
        git(&repo, &["commit", "-m", "base"]);
        git(&repo, &["push", "-u", "origin", "main"]);
    }
}
fn git_command(root: &Path) -> Command {
    let mut c = command(root);
    c.env("PATH", std::env::var_os("PATH").unwrap())
        .env("DOTJIK_REPO", root.join("dotjik"))
        .env("TJIKOS_REPO", root.join("tjikos"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    c
}
#[test]
fn git_push_preserves_dirty_index_and_sync_preflights_every_repo() {
    let d = fixture();
    let r = d.path();
    git_fixture(r);
    let dot = r.join("dotjik");
    let os = r.join("tjikos");
    write(&dot.join("settings"), "commit\n");
    git(&dot, &["commit", "-am", "ahead"]);
    write(&dot.join("settings"), "staged\n");
    git(&dot, &["add", "settings"]);
    write(&dot.join("settings"), "unstaged\n");
    write(&os.join("untracked"), "keep\n");
    let index = fs::read(dot.join(".git/index")).unwrap();
    let tip = git(&r.join("dotjik.git"), &["rev-parse", "main"]);
    assert!(!git_command(r)
        .args(["git", "sync"])
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(git(&r.join("dotjik.git"), &["rev-parse", "main"]), tip);
    success(&git_command(r).args(["git", "push"]).output().unwrap());
    assert_eq!(
        git(&r.join("dotjik.git"), &["rev-parse", "main"]),
        git(&dot, &["rev-parse", "HEAD"])
    );
    assert_eq!(fs::read(dot.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(dot.join("settings")).unwrap(),
        "unstaged\n"
    );
    assert_eq!(fs::read_to_string(os.join("untracked")).unwrap(), "keep\n");
}
#[test]
fn failed_pull_blocks_all_pushes_and_dry_run_has_no_git_mutations() {
    let d = fixture();
    let r = d.path();
    git_fixture(r);
    let dot = r.join("dotjik");
    let os = r.join("tjikos");
    write(&dot.join("settings"), "ahead\n");
    git(&dot, &["commit", "-am", "ahead"]);
    let tip = git(&r.join("dotjik.git"), &["rev-parse", "main"]);
    git(
        &os,
        &[
            "remote",
            "set-url",
            "origin",
            r.join("missing.git").to_str().unwrap(),
        ],
    );
    let index = fs::read(dot.join(".git/index")).unwrap();
    success(
        &git_command(r)
            .args(["git", "sync", "--dry-run"])
            .output()
            .unwrap(),
    );
    assert_eq!(fs::read(dot.join(".git/index")).unwrap(), index);
    assert!(!dot.join(".git/tjikup-workflow.lock").exists());
    assert!(!git_command(r)
        .args(["git", "sync"])
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(git(&r.join("dotjik.git"), &["rev-parse", "main"]), tip);
}
#[test]
fn resolved_rebase_continues_without_opening_editor() {
    let d = fixture();
    let r = d.path();
    git_fixture(r);
    let repo = r.join("dotjik");
    git(&repo, &["checkout", "-b", "side"]);
    write(&repo.join("settings"), "side\n");
    git(&repo, &["commit", "-am", "side message"]);
    git(&repo, &["checkout", "main"]);
    write(&repo.join("settings"), "main\n");
    git(&repo, &["commit", "-am", "main message"]);
    assert!(!Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["-c", "commit.gpgsign=false", "rebase", "side"])
        .output()
        .unwrap()
        .status
        .success());
    write(&repo.join("settings"), "resolved\n");
    git(&repo, &["add", "settings"]);
    success(
        &git_command(r)
            .args(["git", "push"])
            .env("GIT_EDITOR", "false")
            .output()
            .unwrap(),
    );
    assert!(!repo.join(".git/rebase-merge").exists());
    assert_eq!(git(&repo, &["log", "-1", "--format=%s"]), "main message");
    assert_eq!(
        fs::read_to_string(repo.join("settings")).unwrap(),
        "resolved\n"
    );
}
#[test]
fn installer_activates_verified_bytes_repairs_tampering_and_never_downgrades() {
    use sha2::{Digest, Sha256};
    let d = fixture();
    let r = d.path();
    for tool in ["install", "ln", "mv"] {
        symlink(format!("/usr/bin/{tool}"), r.join("bin").join(tool)).unwrap();
    }
    let bytes = "#!/bin/sh\nexit 0\n";
    let digest = format!("{:x}", Sha256::digest(bytes));
    write(&r.join("native/installers.json"),json!({"fixture":{"repository":"owner/tool.name","latest":true,"asset":"tool","kind":"raw","binary":"tool","prefix":"{home}/tool/{version}","links":{"{home}/.local/bin/tool":"{prefix}/tool"}}}).to_string());
    let release = json!({"tag_name":"v1.2.3","assets":[{"name":"tool","browser_download_url":"https://github.com/owner/tool.name/releases/download/v1.2.3/tool","digest":format!("sha256:{digest}")}]});
    executable(&r.join("bin/curl"), &format!("#!/bin/sh\nfor arg; do\n if [ \"$save\" = yes ]; then printf '#!/bin/sh\\nexit 0\\n' > \"$arg\"; exit 0; fi\n [ \"$arg\" != --output ] || save=yes\ndone\nprintf '%s' '{}'\n", release));
    let run = || {
        command(r)
            .args(["workflow", "install", "fixture"])
            .output()
            .unwrap()
    };
    success(&run());
    let target = r.join("home/tool/1.2.3/tool");
    assert_eq!(fs::read_to_string(&target).unwrap(), bytes);
    let out = run();
    success(&out);
    assert!(String::from_utf8_lossy(&out.stdout).contains("unchanged"));
    write(&target, "tampered");
    success(&run());
    assert_eq!(fs::read_to_string(&target).unwrap(), bytes);
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
    success(&run());
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o755
    );
    let newer = r.join("home/tool/2.0.0/tool");
    executable(&newer, "#!/bin/sh\nexit 2\n");
    fs::remove_file(r.join("home/.local/bin/tool")).unwrap();
    symlink(&newer, r.join("home/.local/bin/tool")).unwrap();
    success(&run());
    assert_eq!(
        r.join("home/.local/bin/tool").canonicalize().unwrap(),
        newer
    );
}

#[test]
fn desktop_cleanup_check_is_read_only_and_apply_keeps_recovery_copies() {
    let d = fixture();
    let r = d.path();
    symlink("/usr/bin/cp", r.join("bin/cp")).unwrap();
    executable(
        &r.join("bin/update-desktop-database"),
        "#!/bin/sh\nexit 0\n",
    );
    write(&r.join("native/desktop-maintenance.json"),json!({"launcher_replacements":{"old.desktop":"new.desktop"},"mime_replacements":{},"preserve_launcher_contents":[]}).to_string());
    let apps = r.join("home/.local/share/applications");
    write(&apps.join("old.desktop"), "old launcher");
    write(&apps.join("new.desktop"), "new launcher");
    let mime = r.join("home/.config/mimeapps.list");
    write(&mime, "[Default Applications]\ntext/plain=old.desktop;\n");
    let check = command(r)
        .args(["workflow", "desktop", "cleanup", "--check"])
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1));
    assert!(apps.join("old.desktop").exists());
    assert!(!r.join("home/.local/state").exists());
    let out = command(r)
        .args(["workflow", "desktop", "cleanup", "--apply"])
        .output()
        .unwrap();
    success(&out);
    assert!(!apps.join("old.desktop").exists());
    assert_eq!(
        fs::read_to_string(&mime).unwrap(),
        "[Default Applications]\ntext/plain=new.desktop;\n"
    );
    let record: Value = serde_json::from_slice(&out.stdout).unwrap();
    let backup = Path::new(record["archive"].as_str().unwrap());
    assert_eq!(
        fs::read_to_string(backup.join(".local/share/applications/old.desktop")).unwrap(),
        "old launcher"
    );
    assert!(fs::read_to_string(backup.join(".config/mimeapps.list"))
        .unwrap()
        .contains("old.desktop"));
    success(
        &command(r)
            .args(["workflow", "desktop", "cleanup", "--check"])
            .output()
            .unwrap(),
    );
}
#[test]
fn zed_restores_managed_launcher_after_failed_vendor_install_and_refuses_unexpected_edit() {
    let d = fixture();
    let r = d.path();
    symlink("/usr/bin/cp", r.join("bin/cp")).unwrap();
    let launcher = r.join("home/.local/share/applications/dev.zed.Zed.desktop");
    write(&launcher, "managed launcher\n");
    write(&r.join("native/home/launcher"), "managed launcher\n");
    write(&r.join("native/home/files-device.json"),json!({"version":1,"id":"fixture","entries":[{"kind":"file","path":"{{ user.home }}/.local/share/applications/dev.zed.Zed.desktop","source":"launcher"}]}).to_string());
    write(
        &r.join("home/.local/zed.app/share/applications/dev.zed.Zed.desktop"),
        "vendor launcher\n",
    );
    executable(&r.join("bin/vendor"),"#!/bin/sh\nprintf 'vendor launcher\\n' > \"$HOME/.local/share/applications/dev.zed.Zed.desktop\"\nexit 7\n");
    let out = command(r)
        .args([
            "workflow",
            "desktop",
            "zed",
            "--channel",
            "stable",
            "--",
            "vendor",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(fs::read_to_string(&launcher).unwrap(), "managed launcher\n");
    executable(&r.join("bin/unexpected"),"#!/bin/sh\nprintf 'personal edit\\n' > \"$HOME/.local/share/applications/dev.zed.Zed.desktop\"\n");
    let out = command(r)
        .args([
            "workflow",
            "desktop",
            "zed",
            "--channel",
            "stable",
            "--",
            "unexpected",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(fs::read_to_string(&launcher).unwrap(), "personal edit\n");
    assert!(String::from_utf8_lossy(&out.stderr).contains("backup:"));
}

#[test]
fn icon_retirement_preserves_theme_choice_and_launcher_behavior_with_recovery() {
    let d = fixture();
    let r = d.path();
    for name in ["gtk-update-icon-cache", "update-desktop-database"] {
        executable(&r.join("bin").join(name), "#!/bin/sh\nexit 0\n");
    }
    write(
        &r.join("native/home/files/.local/share/applications/example.desktop"),
        "[Desktop Entry]\nIcon=example\n",
    );
    let app = r.join("home/.local/share/applications/example.desktop");
    let original = "[Desktop Entry]\nExec=example --personal\nIcon=rose-punk-example\n";
    write(&app, original);
    let settings = r.join("config/gtk-3.0/settings.ini");
    let prefs = "[Settings]\ngtk-icon-theme-name=Neuwaita\nuser-setting=keep\n";
    write(&settings, prefs);
    write(
        &r.join("home/.local/share/icons/rose-punk/index.theme"),
        "[Icon Theme]\nName=Rose Punk\n",
    );
    let run = || {
        command(r)
            .args(["workflow", "desktop", "icons"])
            .env_remove("XDG_DATA_HOME")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .output()
            .unwrap()
    };
    success(&run());
    assert_eq!(fs::read_to_string(&settings).unwrap(), prefs);
    assert_eq!(
        fs::read_to_string(&app).unwrap(),
        original.replace("Icon=rose-punk-example", "Icon=example")
    );
    let backup: Value = serde_json::from_slice(
        &fs::read(r.join("state/rose-punk-icons/theme-switch-backups.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(backup["example.desktop"], original);
    write(
        &settings,
        prefs.replace("Neuwaita", "rose-punk-illustrated"),
    );
    success(&run());
    assert_eq!(
        fs::read_to_string(&settings).unwrap(),
        prefs.replace("Neuwaita", "rose-punk")
    );
}
#[test]
fn system_preflights_identity_and_mount_collisions_before_any_mutations() {
    let d = fixture();
    let r = d.path();
    let host = String::from_utf8(Command::new("hostname").output().unwrap().stdout).unwrap();
    let policy = r.join("maintenance.json");
    let fstab = r.join("fstab");
    write(&fstab, "installer-owned\n");
    write(&r.join("storage.json"),json!({"mounts":{"/mnt/data/test":"expected-uuid"},"layout":[["data","d1","/mnt/data/test"]]}).to_string());
    write(&policy,json!({"host":host.trim().split('.').next(),"root_required":false,"provision":{"accounts":[{"name":"fixture","uid":998,"gid":998,"home":"/synthetic","shell":"/bin/false"}],"memberships":[]},"storage":{"config":r.join("storage.json"),"fstab":fstab,"pool":"/mnt/storage","swap":"/mnt/data/test/swap","begin":"# begin","end":"# end","array_prefixes":["/mnt/data/"],"retired_mounts":[],"pool_options":"defaults"}}).to_string());
    executable(&r.join("bin/getent"),"#!/bin/sh\ncase $1 in passwd) echo 'fixture:x:999:998::/synthetic:/bin/false';; group) echo 'fixture:x:998:';; esac\n");
    let out = command(r)
        .args(["system", "--policy"])
        .arg(&policy)
        .args(["provision", "accounts", "--apply"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("collision"));
    executable(
        &r.join("bin/findmnt"),
        "#!/bin/sh\necho 'wrong-uuid ext4'\n",
    );
    let out = command(r)
        .args(["system", "--policy"])
        .arg(&policy)
        .args(["storage", "--write-fstab"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(fs::read_to_string(fstab).unwrap(), "installer-owned\n");
}
