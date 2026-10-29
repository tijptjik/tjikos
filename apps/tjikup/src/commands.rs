//! Public configuration commands and cold-bootstrap file provisioning.
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn home() -> Result<PathBuf> {
    Ok(std::env::var_os("HOME").context("HOME is required")?.into())
}
/// Preserve an existing age identity while adopting the neutral credential path.
/// The previous filename is only consulted here; it is never a public command.
pub fn age_identity(migrate: bool) -> Result<PathBuf> {
    use std::io::{Read, Write};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let home = home()?;
    let target = home.join(".keys/age.txt");
    if target.try_exists()? {
        return Ok(target);
    }
    let previous = home.join(".keys/chezmoi.txt");
    if !migrate {
        return Ok(if previous.is_file() { previous } else { target });
    }
    let mut source = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&previous)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(target),
        Err(e) => return Err(e.into()),
    };
    let info = source.metadata()?;
    if !info.is_file()
        || info.uid() != unsafe { libc::geteuid() }
        || info.mode() & 0o077 != 0
        || info.len() > 65536
    {
        bail!("existing age identity must be a private regular file owned by this user");
    }
    let mut bytes = Vec::new();
    source.read_to_end(&mut bytes)?;
    let mut temporary = tempfile::NamedTempFile::new_in(target.parent().unwrap())?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(&target) {
        Ok(_) => (),
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            if fs::read(&target)? != bytes {
                bail!("age identity changed during migration");
            }
        }
        Err(e) => return Err(e.error.into()),
    }
    Ok(target)
}
pub fn repository() -> PathBuf {
    std::env::var_os("DOTJIK_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
                })
                .join("dotjik")
        })
}
pub fn config(mut args: Vec<String>) -> Result<i32> {
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        println!("tjikup config <home|system> [check|plan|diff|apply|source-path [TARGET]|edit TARGET]\n\nFile operations use the native manifest and preserve conflicting local changes.\nWith no action, print the plan. Tasks and service changes run through tjikup recipes.");
        return Ok(0);
    }
    let scope = args.remove(0);
    if !["home", "system"].contains(&scope.as_str()) {
        bail!("configuration scope must be home or system");
    }
    if args.is_empty() {
        args.push("plan".into());
    }
    let repo = repository();
    if args.len() == 1 && ["check", "plan", "diff", "apply"].contains(&args[0].as_str()) {
        return crate::workflow_files::run(&repo, &[scope, args[0].clone()]);
    }
    if args == ["source-path"] {
        println!(
            "{}",
            repo.join(if scope == "home" {
                "native/home"
            } else {
                "system/native"
            })
            .display()
        );
        return Ok(0);
    }
    if args.len() != 2 || !["source-path", "edit"].contains(&args[0].as_str()) {
        bail!("run tjikup config --help for configuration commands");
    }
    let home = home()?;
    let target = if let Some(relative) = args[1].strip_prefix("~/") {
        home.join(relative)
    } else if Path::new(&args[1]).is_absolute() {
        PathBuf::from(&args[1])
    } else {
        std::env::current_dir()?.join(&args[1])
    };
    let context = crate::workflow::context(&repo)?;
    let host = context["hostname"].as_str().context("hostname missing")?;
    let manifests = if scope == "system" {
        vec![repo.join(format!("system/native/{host}/manifest.json"))]
    } else {
        [format!("files-{host}.json"), format!("{host}.json")]
            .iter()
            .zip(["files-device.json", "device.json"])
            .map(|(specific, fallback)| {
                let p = repo.join("native/home").join(specific);
                if p.is_file() {
                    p
                } else {
                    repo.join("native/home").join(fallback)
                }
            })
            .collect()
    };
    let mut matches = Vec::new();
    for manifest in manifests {
        if !manifest.exists() {
            continue;
        }
        if let Some(source) = crate::managed_files::source_path(&manifest, &context, &target)? {
            matches.push(source);
        }
    }
    matches.sort();
    matches.dedup();
    if matches.len() != 1 {
        bail!(
            "expected one native source for {}; found {}",
            target.display(),
            matches.len()
        );
    }
    let source = &matches[0];
    if args[0] == "source-path" {
        println!("{}", source.display());
        return Ok(0);
    }
    // An editor setting is an explicit user command. Pass the path as a positional
    // argument, never interpolate it into shell source.
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "zed --wait".into());
    if editor.trim().is_empty() {
        bail!("editor is empty");
    }
    Ok(Command::new("sh")
        .args(["-c", &format!("exec {editor} \"$1\""), "tjikup-editor"])
        .arg(source)
        .status()?
        .code()
        .unwrap_or(1))
}

pub fn bootstrap_archives(repo: &Path) -> Result<i32> {
    let home = home()?;
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let mut context = crate::workflow::context(repo)?;
    context["config_dir"] = json!(config);
    let mut missing = false;
    for (key, relative) in [
        ("bootstrap_transport_exists", "synctjik/rclone.conf"),
        ("bootstrap_policy_exists", "archive-policy.json"),
    ] {
        let path = config.join(relative);
        let exists = match fs::symlink_metadata(&path) {
            Ok(info) if info.is_file() => true,
            Ok(_) => bail!(
                "refusing non-regular bootstrap destination: {}",
                path.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        context[key] = json!(exists);
        missing |= !exists;
    }
    if !missing {
        return Ok(0);
    }
    let temporary = tempfile::tempdir()?;
    let context_path = temporary.path().join("context.json");
    fs::write(&context_path, serde_json::to_vec(&context)?)?;
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"))
        .join("tjikup/files/bootstrap");
    Ok(crate::managed_files::cli(vec![
        "apply".into(),
        "--manifest".into(),
        repo.join("native/home/bootstrap.json")
            .to_string_lossy()
            .into(),
        "--context-file".into(),
        context_path.to_string_lossy().into(),
        "--state-dir".into(),
        state.to_string_lossy().into(),
    ]))
}
