//! Repository file-policy selection; deployments remain in the native engine.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
fn home() -> Result<PathBuf> {
    Ok(std::env::var_os("HOME").context("HOME required")?.into())
}
fn system_runtime(apply: bool) -> Result<bool> {
    let source = std::env::current_exe()?;
    let target = Path::new("/usr/local/lib/tjikup/tjikup");
    let desired = fs::read(&source)?;
    if fs::symlink_metadata(target)
        .is_ok_and(|m| m.is_file() && m.uid() == 0 && m.mode() & 0o777 == 0o755)
        && fs::read(target).is_ok_and(|bytes| bytes == desired)
    {
        return Ok(true);
    }
    if !apply {
        return Ok(false);
    }
    let invoke = |args: &[&str]| -> Result<()> {
        if !Command::new("sudo")
            .args(["-n", "--"])
            .args(args)
            .status()?
            .success()
        {
            bail!("system runtime installation failed");
        }
        Ok(())
    };
    invoke(&["install", "-d", "-m", "755", "/usr/local/lib/tjikup"])?;
    let output = Command::new("sudo")
        .args([
            "-n",
            "--",
            "mktemp",
            "/usr/local/lib/tjikup/.runtime.XXXXXXXX",
        ])
        .output()?;
    if !output.status.success() {
        bail!("cannot stage system runtime");
    }
    let temporary = String::from_utf8(output.stdout)?.trim().to_owned();
    let result = (|| -> Result<()> {
        invoke(&[
            "install",
            "-m",
            "755",
            "-o",
            "root",
            "-g",
            "root",
            source.to_str().context("invalid executable path")?,
            &temporary,
        ])?;
        invoke(&["mv", "-Tf", &temporary, target.to_str().unwrap()])
    })();
    if result.is_err() {
        let _ = invoke(&["rm", "-f", &temporary]);
    }
    result?;
    if fs::read(target)? != desired {
        bail!("system runtime verification failed");
    }
    Ok(true)
}
fn migration(retire: bool) -> Result<i32> {
    let mut cmd = Command::new("synctjik");
    cmd.arg("migrate-schedules");
    if retire {
        cmd.arg("--retire");
    }
    let output = cmd.output()?;
    if !output.status.success() {
        eprintln!("Synctjik schedule migration is not ready; retry after running jobs finish.");
    }
    Ok(output.status.code().unwrap_or(1))
}
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            bail!("system policy must not contain symlinks");
        }
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}
pub fn run(repo: &Path, args: &[String]) -> Result<i32> {
    let scope = args.first().context("files requires scope")?;
    let action = args.get(1).map(String::as_str).unwrap_or("plan");
    let action = if action == "diff" { "plan" } else { action };
    if !["check", "plan", "apply"].contains(&action) {
        bail!("unknown files action");
    }
    if action == "apply" {
        crate::commands::age_identity(true)?;
    }
    let group = if args.len() > 2 {
        if args.len() != 4 || args[2] != "--group" {
            bail!("expected --group NAME");
        }
        Some(args[3].as_str())
    } else {
        None
    };
    if scope == "home" {
        let mut nested = args.to_vec();
        nested[0] = "home-files".into();
        let status = run(repo, &nested)?;
        if ![0, 1].contains(&status) || action == "apply" && status != 0 {
            return Ok(status);
        }
        let credentials = run(repo, &["credentials".into(), action.into()])?;
        return Ok(status.max(credentials));
    }
    if !["home-files", "credentials", "system"].contains(&scope.as_str()) {
        bail!("unknown files scope");
    }
    let ctx = crate::workflow::context(repo)?; // Enrollment and dependency validation for every entry point.
    let host = ctx["hostname"].as_str().unwrap().to_owned();
    let home = home()?;
    let state = PathBuf::from(
        std::env::var_os("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state").into()),
    )
    .join("tjikup/files/home");
    let temp = tempfile::tempdir()?;
    let mut values = ctx;
    let mut adopt = false;
    let mut migrate = false;
    let manifest;
    if scope == "home-files" {
        let specific = repo.join(format!("native/home/files-{host}.json"));
        let portable = !specific.is_file();
        manifest = if portable {
            repo.join("native/home/files-device.json")
        } else {
            specific
        };
        if let Some(group) = group {
            let groups: Vec<String> =
                serde_json::from_slice(&fs::read(repo.join("native/home/groups.json"))?)?;
            if !groups.iter().any(|known| known == group) {
                bail!("unknown home file group {group}");
            }
            let entries: Vec<Value> = serde_json::from_slice(&fs::read(repo.join(if portable {
                "native/home/portable-files.json"
            } else {
                "native/home/files.json"
            }))?)?;
            if !entries.iter().any(|e| e["group"] == group) {
                println!("SKIP    - {group}  not enrolled on this device");
                return Ok(0);
            }
        }
        let mut live = json!({});
        for v in [3, 4] {
            let path = home.join(format!(".config/gtk-{v}.0/settings.ini"));
            let contents = if path.is_file() {
                fs::read_to_string(path)?
            } else {
                String::new()
            };
            live[format!("gtk{v}_icon_theme")] = json!(contents
                .lines()
                .find_map(|l| l.strip_prefix("gtk-icon-theme-name="))
                .unwrap_or_default());
        }
        values["live"] = live;
        adopt = !portable;
        if action == "apply" && group.is_none_or(|g| g == "base") {
            let units = home.join(".config/systemd/user");
            migrate = [
                "backup-packages",
                "synctjik-device-collections-backup",
                "cloudflare-restic-backup",
                "private-backup",
                "private-sync",
            ]
            .iter()
            .any(|n| {
                [".service", ".timer"]
                    .iter()
                    .any(|ext| units.join(format!("{n}{ext}")).symlink_metadata().is_ok())
            });
            if migrate && migration(false)? != 0 {
                return Ok(2);
            }
        }
    } else if scope == "credentials" {
        let specific = repo.join(format!("native/home/{host}.json"));
        manifest = if specific.is_file() {
            specific
        } else {
            repo.join("native/home/device.json")
        };
        adopt = ["fi", "li", "si"].contains(&host.as_str());
        if adopt && action == "apply" {
            let token = home.join(".config/bws/environment");
            if let Ok(info) = token.symlink_metadata() {
                if !info.is_file() || info.uid() != unsafe { libc::geteuid() } {
                    bail!("Bitwarden token must be a regular file owned by this user");
                }
                fs::set_permissions(token, fs::Permissions::from_mode(0o600))?;
            }
        }
    } else {
        let base = repo.join(format!("system/native/{host}"));
        if !base.join("manifest.json").is_file() {
            println!("SKIP: no machine-specific system files enrolled");
            return Ok(0);
        }
        if action == "check" && !system_runtime(false)? {
            return Ok(1);
        }
        if action == "apply" {
            system_runtime(true)?;
        }
        let mapping = base.join("encrypted.json");
        let encrypted: std::collections::BTreeMap<String, String> = if mapping.exists() {
            serde_json::from_slice(&fs::read(mapping)?)?
        } else {
            Default::default()
        };
        if encrypted.is_empty() {
            manifest = base.join("manifest.json");
        } else {
            let policy = temp.path().join("policy");
            copy_tree(&base, &policy)?;
            manifest = policy.join("manifest.json");
            for (name, source) in encrypted {
                for relative in [&name, &source] {
                    if Path::new(relative).is_absolute()
                        || Path::new(relative)
                            .components()
                            .any(|c| c == std::path::Component::ParentDir)
                    {
                        bail!("invalid encrypted policy path");
                    }
                }
                let result = Command::new("age")
                    .args(["--decrypt", "--identity"])
                    .arg(crate::commands::age_identity(action == "apply")?)
                    .arg(base.join(source))
                    .output()?;
                if !result.status.success() {
                    bail!("system input decryption failed; check age identity");
                }
                let destination = policy.join(name);
                fs::create_dir_all(destination.parent().unwrap())?;
                fs::write(&destination, result.stdout)?;
                fs::set_permissions(destination, fs::Permissions::from_mode(0o600))?;
            }
        }
        values = json!({});
        if ["li", "si"].contains(&host.as_str()) {
            let release = fs::read_to_string("/etc/os-release")?;
            let field = |key: &str| {
                release
                    .lines()
                    .find_map(|l| l.strip_prefix(&format!("{key}=")))
                    .unwrap_or_default()
                    .trim_matches('"')
            };
            values = json!({"os":"linux","distribution":field("ID"),"release":field("VERSION_ID").split('.').next().unwrap_or("0").parse::<u64>()?});
        }
    }
    let spec: Value =
        serde_json::from_slice(&fs::read(&manifest).context("missing native manifest")?)?;
    if adopt
        && action == "check"
        && !state
            .join(format!(
                "{}.json",
                spec["id"].as_str().context("manifest id required")?
            ))
            .exists()
    {
        return Ok(1);
    }
    let context = temp.path().join("context.json");
    fs::write(&context, serde_json::to_vec(&values)?)?;
    let state = if scope == "system" {
        PathBuf::from("/var/lib/tjikup/files")
    } else {
        state
    };
    let mut native = vec![
        action.into(),
        "--manifest".into(),
        manifest.to_string_lossy().into_owned(),
        "--state-dir".into(),
        state.to_string_lossy().into_owned(),
        "--context-file".into(),
        context.to_string_lossy().into_owned(),
    ];
    if let Some(group) = group {
        native.extend(["--group".into(), group.into()]);
    }
    if adopt && action == "apply" {
        native.push("--adopt-identical".into());
    }
    let result = if scope == "system" {
        Command::new("sudo")
            .args(["-n", "--"])
            .arg(std::env::current_exe()?)
            .arg("files")
            .args(native)
            .status()?
            .code()
            .unwrap_or(1)
    } else {
        crate::managed_files::cli(native)
    };
    if result == 0 && migrate {
        return migration(true);
    }
    Ok(result)
}
