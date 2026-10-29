//! Verified application installers. Product choices and build steps live in policy.
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    repository: String,
    #[serde(default)]
    asset: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    binary: String,
    #[serde(default)]
    prefix: String,
    #[serde(default)]
    links: BTreeMap<String, String>,
    #[serde(default)]
    check: Vec<String>,
    #[serde(default)]
    steps: Vec<Vec<String>>,
    #[serde(default)]
    packages: Vec<String>,
    #[serde(default)]
    privileged: bool,
    #[serde(default)]
    latest: bool,
    #[serde(default)]
    user_alias: Option<String>,
}
pub fn version(value: &str) -> Option<[u64; 3]> {
    let parts: Vec<_> = value
        .strip_prefix('v')
        .unwrap_or(value)
        .split('.')
        .map(str::parse::<u64>)
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    parts.try_into().ok()
}
fn run(command: &mut Command) -> Result<()> {
    if !command.status()?.success() {
        bail!("installer command failed");
    }
    Ok(())
}
fn admin(policy: &Policy, program: &str) -> Command {
    let mut cmd = if policy.privileged {
        let mut c = Command::new("sudo");
        c.args(["-n", "--", program]);
        c
    } else {
        Command::new(program)
    };
    cmd.stdin(Stdio::null());
    cmd
}
fn expand(value: &str, vars: &BTreeMap<&str, String>) -> String {
    vars.iter().fold(value.to_owned(), |s, (k, v)| {
        s.replace(&format!("{{{k}}}"), v)
    })
}
fn token() -> Option<String> {
    for key in ["GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(t) = std::env::var(key) {
            if !t.trim().is_empty() {
                return Some(t.trim().into());
            }
        }
    }
    let mut command = Command::new("gh");
    if unsafe { libc::geteuid() } == 0 {
        if let Ok(user) = std::env::var("SUDO_USER") {
            if !user.is_empty() && user != "root" {
                command = Command::new("sudo");
                command.args([
                    "-n",
                    "-H",
                    "-u",
                    &user,
                    "--",
                    "env",
                    "-u",
                    "XDG_CONFIG_HOME",
                    "-u",
                    "GH_CONFIG_DIR",
                    "gh",
                ]);
            }
        }
    }
    command
        .args(["auth", "token", "--hostname", "github.com"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
}
fn metadata(url: &str) -> Result<Value> {
    if !url.starts_with("https://api.github.com/repos/") {
        bail!("unexpected metadata origin");
    }
    let mut command = Command::new("curl");
    command.args([
        "--proto",
        "=https",
        "--silent",
        "--show-error",
        "--fail",
        "--max-time",
        "60",
        "--user-agent",
        "tjikup-installer",
        "--header",
        "Accept: application/vnd.github+json",
    ]);
    // Metadata redirects are not followed. Credentials never reach asset hosts.
    let mut header = None;
    if let Some(token) = token() {
        if token.contains(['\r', '\n']) {
            bail!("invalid GitHub credential");
        }
        let mut file = tempfile::NamedTempFile::new()?;
        writeln!(file, "Authorization: Bearer {token}")?;
        command.args(["--header", &format!("@{}", file.path().display())]);
        header = Some(file);
    }
    let result = command.arg(url).output()?;
    drop(header);
    if !result.status.success() {
        bail!("GitHub metadata lookup failed; check network access and GitHub authentication");
    }
    Ok(serde_json::from_slice(&result.stdout).context("invalid GitHub release metadata")?)
}
fn release(policy: &Policy) -> Result<Value> {
    if policy.repository.split('/').count() != 2
        || !policy
            .repository
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_/ .".contains(&c))
        || policy.repository.contains([' ', '.'])
    {
        bail!("invalid release repository");
    }
    let url = format!(
        "https://api.github.com/repos/{}/releases{}",
        policy.repository,
        if policy.latest {
            "/latest"
        } else {
            "?per_page=100"
        }
    );
    let response = metadata(&url)?;
    let entries = if policy.latest {
        vec![response]
    } else {
        response
            .as_array()
            .context("release list required")?
            .clone()
    };
    entries
        .into_iter()
        .find(|e| {
            e["draft"] != true
                && e["prerelease"] != true
                && e["tag_name"].as_str().and_then(version).is_some()
        })
        .context("no stable semantic-version release found")
}
fn digest(asset: &Value) -> Result<String> {
    let text = asset["digest"]
        .as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .context("release has no SHA-256 digest")?;
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("invalid release digest");
    }
    Ok(text.to_ascii_lowercase())
}
fn activate(policy: &Policy, link: &Path, target: &Path) -> Result<bool> {
    if let Ok(meta) = fs::symlink_metadata(link) {
        if !meta.file_type().is_symlink() {
            bail!("refusing unmanaged command: {}", link.display());
        }
    }
    if link.canonicalize().ok() == target.canonicalize().ok() && target.exists() {
        return Ok(false);
    }
    run(admin(policy, "install")
        .args(["-d", "-m", "755"])
        .arg(link.parent().context("link parent")?))?;
    let temporary = link.with_file_name(format!(
        ".{}.tjikup-{}",
        link.file_name().unwrap().to_string_lossy(),
        std::process::id()
    ));
    run(admin(policy, "ln").arg("-s").arg(target).arg(&temporary))?;
    let result = run(admin(policy, "mv").arg("-Tf").arg(&temporary).arg(link));
    if result.is_err() {
        let _ = admin(policy, "rm").arg("-f").arg(&temporary).status();
    }
    result?;
    Ok(true)
}
fn extract_binary(archive: &Path, name: &str, target: &Path) -> Result<()> {
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(archive)?));
    let mut found = false;
    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            bail!("unsafe release archive path");
        }
        if path.file_name().is_some_and(|n| n == name) {
            if found || !entry.header().entry_type().is_file() {
                bail!("expected exactly one regular release executable");
            }
            if entry.size() > 256 * 1024 * 1024 {
                bail!("release executable too large");
            }
            let mut out = fs::File::create(target)?;
            std::io::copy(&mut entry, &mut out)?;
            found = true;
        }
    }
    if !found {
        bail!("release executable missing");
    }
    Ok(())
}
fn install(repo: &Path, name: &str, metadata_only: bool) -> Result<bool> {
    let specs: BTreeMap<String, Policy> =
        serde_json::from_slice(&fs::read(repo.join("native/installers.json"))?)?;
    let policy = specs.get(name).context("unknown installer policy")?;
    let home = crate::commands::home()?;
    let mut vars = BTreeMap::from([
        ("home", home.to_string_lossy().into_owned()),
        ("repo", repo.to_string_lossy().into_owned()),
        ("arch", std::env::consts::ARCH.to_owned()),
        (
            "arm_suffix",
            if std::env::consts::ARCH == "aarch64" {
                "-arm64".into()
            } else {
                String::new()
            },
        ),
    ]);
    if std::env::consts::ARCH != "x86_64" && name != "obsidian" {
        bail!("this installer policy supports x86_64 only");
    }
    let declared_links: Vec<_> = policy
        .links
        .keys()
        .map(|l| PathBuf::from(expand(l, &vars)))
        .collect();
    let present = declared_links.first().is_some_and(|p| p.is_file());
    let operation = std::env::var("TJIKUP_OPERATION").unwrap_or_default();
    if !metadata_only
        && ((operation == "install" && present) || (operation == "update" && !present))
    {
        return Ok(false);
    }
    for link in &declared_links {
        if let Ok(meta) = fs::symlink_metadata(link) {
            if !meta.file_type().is_symlink() {
                bail!("refusing unmanaged command: {}", link.display());
            }
        }
    }
    let release = release(policy)?;
    let tag = release["tag_name"]
        .as_str()
        .context("release tag missing")?;
    let v = tag.trim_start_matches('v');
    vars.insert("version", v.into());
    vars.insert("tag", tag.into());
    let asset_name = expand(&policy.asset, &vars);
    let asset = release["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["name"] == asset_name));
    if metadata_only {
        println!("{v}|{}", digest(asset.context("release asset missing")?)?);
        return Ok(false);
    }
    let mut prefix = PathBuf::from(expand(&policy.prefix, &vars));
    let mut old_version = None;
    if let Some(link) = declared_links.first() {
        if let Ok(target) = link.canonicalize() {
            if let Some(parent) = target.parent() {
                let old = parent
                    .file_name()
                    .and_then(|s| s.to_str())
                    .and_then(version);
                if parent.parent() == prefix.parent() {
                    old_version = old;
                    if old > version(v) {
                        prefix = parent.into();
                    }
                }
            }
        }
    }
    vars.insert("prefix", prefix.to_string_lossy().into_owned());
    let temp = tempfile::tempdir()?;
    vars.insert("temp", temp.path().to_string_lossy().into_owned());
    let mut changed = false;
    if old_version <= version(v) {
        let mut marker = String::new();
        let target = prefix.join(expand(&policy.binary, &vars));
        if policy.kind == "git-build" {
            let revision = metadata(&format!(
                "https://api.github.com/repos/{}/commits/{tag}",
                policy.repository
            ))?["sha"]
                .as_str()
                .context("missing commit identity")?
                .to_owned();
            if revision.len() != 40 || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
                bail!("invalid source revision");
            }
            marker = revision.clone();
            if fs::read_to_string(prefix.join(".installed"))
                .unwrap_or_default()
                .trim()
                != revision
                || !target.is_file()
            {
                let missing: Vec<_> = policy
                    .packages
                    .iter()
                    .filter(|p| {
                        !Command::new("rpm")
                            .args(["-q", p])
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status()
                            .is_ok_and(|s| s.success())
                    })
                    .collect();
                if !missing.is_empty() {
                    run(admin(policy, "dnf").args(["install", "-y"]).args(missing))?;
                }
                let source = temp.path().join("source");
                vars.insert("source", source.to_string_lossy().into_owned());
                let stage = temp.path().join("install");
                vars.insert("stage", stage.to_string_lossy().into_owned());
                run(Command::new("git")
                    .args([
                        "clone",
                        "--quiet",
                        "--depth",
                        "1",
                        "--branch",
                        tag,
                        &format!("https://github.com/{}.git", policy.repository),
                    ])
                    .arg(&source))?;
                let actual = Command::new("git")
                    .arg("-C")
                    .arg(&source)
                    .args(["rev-parse", "HEAD"])
                    .output()?;
                if !actual.status.success() || String::from_utf8(actual.stdout)?.trim() != revision
                {
                    bail!("release tag changed during download");
                }
                for step in &policy.steps {
                    if step.is_empty() {
                        bail!("empty build step");
                    }
                    let args: Vec<_> = step.iter().map(|s| expand(s, &vars)).collect();
                    run(Command::new(&args[0]).args(&args[1..]).current_dir(&source))?;
                }
                if !stage.is_dir() {
                    bail!("build did not produce staged installation");
                }
                run(admin(policy, "install")
                    .args(["-d", "-m", "755"])
                    .arg(&prefix))?;
                run(admin(policy, "cp")
                    .arg("-a")
                    .arg(stage.join("."))
                    .arg(&prefix))?;
                if policy.privileged {
                    run(admin(policy, "chown")
                        .args(["-R", "root:root"])
                        .arg(&prefix))?;
                }
                changed = true;
            }
        } else {
            let asset = asset.context("release asset missing")?;
            let expected = digest(asset)?;
            let url = asset["browser_download_url"]
                .as_str()
                .context("asset URL missing")?;
            if url
                != format!(
                    "https://github.com/{}/releases/download/{tag}/{asset_name}",
                    policy.repository
                )
            {
                bail!("unexpected release asset origin");
            }
            let current = fs::read_to_string(prefix.join(".installed")).unwrap_or_default();
            if !target.is_file() || current.trim() != expected {
                let download = temp.path().join("download");
                run(Command::new("curl")
                    .args([
                        "--proto",
                        "=https",
                        "--proto-redir",
                        "=https",
                        "--fail",
                        "--location",
                        "--silent",
                        "--show-error",
                        "--retry",
                        "3",
                        "--connect-timeout",
                        "20",
                        "--max-time",
                        "600",
                        "--output",
                    ])
                    .arg(&download)
                    .arg(url))?;
                let mut file = fs::File::open(&download)?;
                let mut hasher = Sha256::new();
                let mut buf = [0u8; 65536];
                loop {
                    let count = file.read(&mut buf)?;
                    if count == 0 {
                        break;
                    }
                    hasher.update(&buf[..count]);
                }
                if format!("{:x}", hasher.finalize()) != expected
                    || asset["size"].as_u64().is_some_and(|s| {
                        fs::metadata(&download)
                            .map(|m| m.len() != s)
                            .unwrap_or(true)
                    })
                {
                    bail!("release checksum or size mismatch");
                }
                let candidate = if policy.kind == "tar.gz" {
                    let p = temp.path().join("binary");
                    extract_binary(&download, &policy.binary, &p)?;
                    p
                } else if policy.kind == "raw" {
                    download
                } else {
                    bail!("unknown asset format");
                };
                fs::set_permissions(&candidate, fs::Permissions::from_mode(0o755))?;
                if !policy.check.is_empty() {
                    run(Command::new(&candidate)
                        .args(&policy.check)
                        .stdout(Stdio::null()))?;
                }
                run(admin(policy, "install")
                    .args(["-d", "-m", "755"])
                    .arg(&prefix))?;
                let staged = target.with_extension(format!("tjikup-{}", std::process::id()));
                run(admin(policy, "install")
                    .args(["-m", "755"])
                    .arg(&candidate)
                    .arg(&staged))?;
                run(admin(policy, "mv").arg("-f").arg(&staged).arg(&target))?;
                marker = expected;
                changed = true;
            }
        }
        if changed && !marker.is_empty() {
            let file = temp.path().join("marker");
            fs::write(&file, format!("{marker}\n"))?;
            run(admin(policy, "install")
                .args(["-m", "644"])
                .arg(file)
                .arg(prefix.join(".installed")))?;
        }
    }
    for (link, target) in &policy.links {
        let link = PathBuf::from(expand(link, &vars));
        let target = PathBuf::from(expand(target, &vars));
        if !target.is_file() {
            bail!("installed executable missing: {}", target.display());
        }
        changed |= activate(policy, &link, &target)?;
    }
    if let Some(alias) = &policy.user_alias {
        let link = home.join(".local/bin").join(alias);
        let target = PathBuf::from("/usr/local/bin").join(alias);
        if link.canonicalize().ok() != target.canonicalize().ok() {
            fs::create_dir_all(link.parent().unwrap())?;
            if let Ok(meta) = fs::symlink_metadata(&link) {
                if !meta.file_type().is_symlink() {
                    let backup = home.join(".local/state/tjikup/installers").join(alias);
                    fs::create_dir_all(&backup)?;
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_nanos();
                    fs::copy(&link, backup.join(format!("{alias}-{stamp}")))?;
                }
            }
            let temporary = link.with_extension(format!("tjikup-{}", std::process::id()));
            symlink(&target, &temporary)?;
            fs::rename(&temporary, &link)?;
            changed = true;
        }
    }
    Ok(changed)
}
pub fn cli(repo: &Path, args: &[String]) -> Result<i32> {
    if args.len() == 3 && args[0] == "newer" {
        let latest = version(&args[1]).context("invalid release version")?;
        println!(
            "{}",
            if version(&args[2]).is_none_or(|v| latest > v) {
                "yes"
            } else {
                "no"
            }
        );
        return Ok(0);
    }
    let metadata_only = args.last().is_some_and(|s| s == "--metadata");
    if args.len() != if metadata_only { 2 } else { 1 } {
        bail!("install NAME [--metadata], or install newer LATEST CURRENT");
    }
    let changed = install(repo, &args[0], metadata_only)?;
    if !metadata_only {
        println!(
            "TJIKUP_RESULT={}",
            if changed { "changed" } else { "unchanged" }
        );
        println!(
            "__stage_note__:{}",
            if changed { "updated" } else { "no changes" }
        );
    }
    Ok(0)
}
