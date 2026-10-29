//! Native system maintenance; all machine-specific selection stays in policy.
mod media;
mod permissions;
mod storage;
mod updates;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};
pub fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .with_context(|| format!("missing maintenance policy field {key}"))
}
pub fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
pub fn command(args: &[&str]) -> Result<String> {
    if args.is_empty() {
        bail!("empty command");
    }
    crate::command_io::checked(Command::new(args[0]).args(&args[1..]), 120)
}
pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    if path.is_symlink() {
        bail!("private state cannot be a symlink");
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub fn atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().context("missing destination parent")?)?;
    temporary.write_all(bytes)?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    fs::File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}
pub fn stamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%S%.9fZ").to_string()
}
pub fn receipt(path: &Path, value: &Value) -> Result<()> {
    atomic(path, &serde_json::to_vec_pretty(value)?, 0o600)
}
pub fn check_host(policy: &Value) -> Result<()> {
    if crate::device::hostname()?.split('.').next() != Some(text(policy, "host")?) {
        bail!("maintenance policy belongs to a different host");
    }
    if policy["root_required"] == true && unsafe { libc::geteuid() } != 0 {
        bail!("this maintenance policy requires root");
    }
    Ok(())
}
fn nss(policy: &Value, apply: bool) -> Result<i32> {
    let target = Path::new(text(&policy["nss"], "target")?);
    let original = fs::read_to_string(target)?;
    let updated = storage::nss_render(&original);
    if original == updated {
        println!("systemd shadow lookups already configured");
        return Ok(0);
    }
    if !apply {
        println!("Add systemd to shadow and gshadow lookups");
        return Ok(0);
    }
    if target.is_symlink() {
        bail!("use authselect for a managed nsswitch symlink");
    }
    let info = fs::metadata(target)?;
    let backup = Path::new(text(&policy["nss"], "backup")?);
    private_dir(backup.parent().unwrap())?;
    if !backup.exists() {
        command(&[
            "cp",
            "--preserve=all",
            "--",
            target.to_str().unwrap(),
            backup.to_str().unwrap(),
        ])?;
    }
    let mut temp = tempfile::NamedTempFile::new_in(target.parent().unwrap())?;
    temp.write_all(updated.as_bytes())?;
    temp.as_file()
        .set_permissions(fs::Permissions::from_mode(info.mode() & 0o7777))?;
    use std::os::fd::AsRawFd;
    if unsafe { libc::fchown(temp.as_file().as_raw_fd(), info.uid(), info.gid()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    temp.as_file().sync_all()?;
    temp.persist(target).map_err(|e| e.error)?;
    command(&["restorecon", target.to_str().unwrap()])?;
    println!("systemd shadow lookups configured");
    Ok(0)
}
pub fn cli(mut args: Vec<String>) -> Result<i32> {
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        println!("tjikup system [--policy PATH] COMMAND\n\n  nss [--apply]                       Preserve NSS policy and enable shadow lookup\n  storage --check|--check-docker|--print-fstab|--write-fstab\n  provision accounts|directories|mergerfs|beets [--apply]\n  permissions [--apply]               Journal changes within declared media roots\n  media-guard [--dry-run]             Reject executable torrent payloads\n  transmission-cleanup [--apply|--dry-run]\n  update-apps                        Update running services behind backup gates\n\nPolicy defaults to /etc/tjikup/maintenance.json. Host and privilege checks apply.");
        return Ok(0);
    }
    let path = if args.first().is_some_and(|s| s == "--policy") {
        if args.len() < 3 {
            bail!("--policy requires path and command");
        }
        let path = PathBuf::from(args.remove(1));
        args.remove(0);
        path
    } else {
        PathBuf::from("/etc/tjikup/maintenance.json")
    };
    let policy = read(&path)?;
    check_host(&policy)?;
    match args[0].as_str() {
        "nss" if args.len() == 1 || args[1..] == ["--apply"] => nss(&policy, args.len() == 2),
        "storage" => storage::cli(&policy, &args[1..]),
        "provision" => storage::provision(&policy, &args[1..]),
        "permissions" if args.len() == 1 || args[1..] == ["--apply"] => {
            permissions::run(&policy, args.len() == 2)
        }
        "media-guard" if args.len() == 1 || args[1..] == ["--dry-run"] => {
            media::guard(&policy, args.len() == 2)
        }
        "transmission-cleanup"
            if args.len() == 1 || args[1..] == ["--apply"] || args[1..] == ["--dry-run"] =>
        {
            media::cleanup(&policy, args.get(1).is_some_and(|s| s == "--apply"))
        }
        "update-apps" if args.len() == 1 => updates::run(&policy),
        _ => bail!("run tjikup system --help for usage"),
    }
}
