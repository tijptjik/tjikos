//! Verified Bitwarden release installation. No interpreter payloads.
use anyhow::{bail, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug)]
struct LookupUnavailable;
impl std::fmt::Display for LookupUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "release metadata lookup failed; check network access or GitHub authentication"
        )
    }
}
impl std::error::Error for LookupUnavailable {}

fn version(value: &str) -> Option<Vec<u64>> {
    let parts = value
        .split('.')
        .map(str::parse)
        .collect::<std::result::Result<Vec<u64>, _>>()
        .ok()?;
    (parts.len() == 3).then_some(parts)
}
fn current(tool: &str) -> Result<Option<(String, PathBuf)>> {
    let link = PathBuf::from(format!("/usr/local/bin/{tool}"));
    if !link.is_symlink() {
        if link.exists() {
            bail!("refusing to overwrite unmanaged {tool} executable");
        }
        return Ok(None);
    }
    let Ok(target) = link.canonicalize() else {
        return Ok(None);
    };
    if target.parent().and_then(Path::parent) != Some(Path::new(&format!("/opt/{tool}"))) {
        return Ok(None);
    }
    let name = target
        .parent()
        .and_then(Path::file_name)
        .context("invalid managed install path")?
        .to_string_lossy()
        .trim_start_matches('v')
        .to_owned();
    Ok(Some((name, target)))
}
fn recorded(tool: &str) -> Result<bool> {
    let Some((v, path)) = current(tool)? else {
        return Ok(false);
    };
    let digest = fs::read_to_string(path.parent().unwrap().join(".installed")).unwrap_or_default();
    Ok(version(&v).is_some()
        && path.is_file()
        && fs::metadata(path)?.permissions().mode() & 0o111 != 0
        && valid_digest(digest.trim()))
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn curl(url: &str, head: bool, github: bool) -> Result<Vec<u8>> {
    let mut cmd = Command::new("curl");
    cmd.args([
        "--silent",
        "--show-error",
        "--fail",
        "--max-time",
        "60",
        "--proto",
        "=https",
        "--user-agent",
        "tjikup-bitwarden",
    ]);
    if head {
        cmd.arg("--head");
    } // Never follow metadata redirects with credentials.
    let mut header = None;
    if github {
        let token = std::env::var("GH_TOKEN")
            .or_else(|_| std::env::var("GITHUB_TOKEN"))
            .ok()
            .or_else(|| {
                Command::new("gh")
                    .args(["auth", "token", "--hostname", "github.com"])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            });
        if let Some(token) = token.filter(|s| !s.is_empty()) {
            if token.contains(['\n', '\r']) {
                bail!("invalid GitHub credential");
            }
            let mut file = tempfile::NamedTempFile::new()?;
            writeln!(file, "Authorization: Bearer {token}")?;
            cmd.arg("--header")
                .arg(format!("@{}", file.path().display()));
            header = Some(file);
        }
    }
    let output = cmd.arg(url).output()?;
    drop(header);
    if !output.status.success() {
        return Err(LookupUnavailable.into());
    }
    Ok(output.stdout)
}
fn release(tool: &str) -> Result<(String, String, String)> {
    let (repo, tag, name, v) = if tool == "bws" {
        let data: Value =
            serde_json::from_slice(&curl("https://crates.io/api/v1/crates/bws", false, false)?)?;
        let c = &data["crate"];
        if c["name"] != "bws"
            || c["repository"]
                .as_str()
                .unwrap_or_default()
                .trim_end_matches(".git")
                != "https://github.com/bitwarden/sdk-sm"
        {
            bail!("unexpected bws package identity");
        }
        let v = c["max_stable_version"]
            .as_str()
            .context("missing stable bws version")?
            .to_owned();
        (
            "bitwarden/sdk-sm",
            format!("bws-v{v}"),
            format!("bws-x86_64-unknown-linux-gnu-{v}.zip"),
            v,
        )
    } else {
        let head = String::from_utf8(curl(
            "https://bitwarden.com/download/?app=cli&platform=linux",
            true,
            false,
        )?)?;
        let location = head
            .lines()
            .find_map(|l| {
                l.split_once(':')
                    .filter(|(k, _)| k.eq_ignore_ascii_case("location"))
                    .map(|(_, v)| v.trim())
            })
            .context("missing Bitwarden release redirect")?;
        let rest = location
            .strip_prefix("https://github.com/bitwarden/clients/releases/download/cli-v")
            .context("unexpected Bitwarden release URL")?;
        let (v, file) = rest
            .split_once('/')
            .context("invalid Bitwarden release URL")?;
        if file != format!("bw-linux-{v}.zip") {
            bail!("inconsistent Bitwarden release URL");
        }
        (
            "bitwarden/clients",
            format!("cli-v{v}"),
            file.to_owned(),
            v.to_owned(),
        )
    };
    if version(&v).is_none() {
        bail!("invalid stable release version");
    }
    let data: Value = serde_json::from_slice(&curl(
        &format!("https://api.github.com/repos/{repo}/releases/tags/{tag}"),
        false,
        true,
    )?)?;
    if data["tag_name"] != tag || data["draft"] != false || data["prerelease"] != false {
        bail!("release is not the expected stable version");
    }
    let asset = data["assets"]
        .as_array()
        .context("missing release assets")?
        .iter()
        .find(|a| a["name"] == name)
        .context("expected binary archive missing")?;
    let digest = asset["digest"]
        .as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .filter(|s| valid_digest(s))
        .context("missing SHA-256 release digest")?;
    let url = asset["browser_download_url"]
        .as_str()
        .context("missing asset URL")?;
    if url != format!("https://github.com/{repo}/releases/download/{tag}/{name}") {
        bail!("unexpected asset download URL");
    }
    Ok((v, url.into(), digest.to_lowercase()))
}
fn run(cmd: &mut Command) -> Result<()> {
    if !cmd
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success()
    {
        bail!("Bitwarden installation command failed");
    }
    Ok(())
}
fn usable(tool: &str) -> Result<bool> {
    if !recorded(tool)? {
        return Ok(false);
    }
    let Some((v, path)) = current(tool)? else {
        return Ok(false);
    };
    let temp = tempfile::tempdir()?;
    let output = Command::new("timeout")
        .arg("10")
        .arg(path)
        .arg("--version")
        .env("BITWARDENCLI_APPDATA_DIR", temp.path())
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(output.status.success()
        && [v.clone(), format!("{tool} {v}")].contains(&text.trim().to_owned()))
}
fn install(tool: &str, v: &str, url: &str, digest: &str) -> Result<bool> {
    let existing = current(tool)?;
    if existing
        .as_ref()
        .is_some_and(|(old, _)| version(old) > version(v))
    {
        return Ok(false);
    }
    let dir = existing
        .as_ref()
        .filter(|(old, _)| old == v)
        .map(|(_, p)| p.parent().unwrap().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(format!("/opt/{tool}/{v}")));
    let target = dir.join(tool);
    let mut changed = false;
    if !recorded(tool)?
        || !target.is_file()
        || fs::read_to_string(dir.join(".installed"))
            .unwrap_or_default()
            .trim()
            != digest
    {
        let temp = tempfile::tempdir()?;
        let archive = temp.path().join("release.zip");
        run(Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--fail",
                "--location",
                "--max-time",
                "120",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--output",
            ])
            .arg(&archive)
            .arg(url))?;
        if format!("{:x}", Sha256::digest(fs::read(&archive)?)) != digest {
            bail!("SHA-256 mismatch for {tool}");
        }
        let listing = Command::new("unzip").arg("-Z1").arg(&archive).output()?;
        if !listing.status.success() {
            bail!("invalid release archive");
        }
        let text = String::from_utf8(listing.stdout)?;
        let members: Vec<_> = text
            .lines()
            .filter(|s| Path::new(s).file_name().is_some_and(|n| n == tool))
            .collect();
        if members.len() != 1
            || members[0].starts_with('/')
            || members[0].split('/').any(|p| p == "..")
        {
            bail!("expected one safe binary in release archive");
        }
        // Stream the chosen member to a fixed private file; never extract archive paths.
        let binary = temp.path().join(tool);
        let out = fs::File::create(&binary)?;
        if !Command::new("unzip")
            .arg("-p")
            .arg(&archive)
            .arg(members[0])
            .stdout(out)
            .stderr(Stdio::null())
            .status()?
            .success()
        {
            bail!("binary extraction failed");
        }
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700))?;
        run(Command::new("timeout")
            .arg("10")
            .arg(&binary)
            .arg("--version")
            .env("BITWARDENCLI_APPDATA_DIR", temp.path().join("check")))?;
        run(Command::new("sudo")
            .args(["install", "-d", "-m", "755"])
            .arg(&dir))?;
        let staged = dir.join(format!("{tool}.new"));
        run(Command::new("sudo")
            .args(["install", "-m", "755"])
            .arg(&binary)
            .arg(&staged))?;
        run(Command::new("sudo")
            .args(["mv", "-f"])
            .arg(&staged)
            .arg(&target))?;
        let marker = temp.path().join(".installed");
        fs::write(&marker, format!("{digest}\n"))?;
        run(Command::new("sudo")
            .args(["install", "-m", "644"])
            .arg(marker)
            .arg(dir.join(".installed")))?;
        changed = true;
    }
    let link = PathBuf::from(format!("/usr/local/bin/{tool}"));
    current(tool)?; // Recheck ownership before publication.
    if link.canonicalize().ok().as_ref() != Some(&target) {
        let stage = PathBuf::from(format!("/usr/local/bin/{tool}.tjikup-new"));
        run(Command::new("sudo")
            .args(["ln", "-sfn"])
            .arg(&target)
            .arg(&stage))?;
        run(Command::new("sudo")
            .args(["mv", "-Tf"])
            .arg(stage)
            .arg(link))?;
        changed = true;
    }
    Ok(changed)
}
pub fn cli(args: &[String]) -> Result<i32> {
    if args.iter().any(|a| a != "--upgrade") {
        bail!("bitwarden [--upgrade]");
    }
    if std::env::consts::ARCH != "x86_64" {
        bail!("Bitwarden release installation currently supports x86_64 only");
    }
    let upgrade = args.iter().any(|a| a == "--upgrade");
    let update = std::env::var("TJIKUP_OPERATION").as_deref() == Ok("update");
    let mut changed = false;
    let mut deferred = false;
    for tool in ["bws", "bw"] {
        let installed = recorded(tool)?;
        if !upgrade && installed || update && !installed {
            continue;
        }
        match release(tool) {
            Ok((v, url, digest)) => changed |= install(tool, &v, &url, &digest)?,
            Err(error) => {
                if error.is::<LookupUnavailable>() && usable(tool)? {
                    deferred = true;
                } else {
                    return Err(error);
                }
            }
        }
    }
    if deferred {
        println!("CHECK   ! Bitwarden CLIs  release check deferred; managed installs retained");
        return Ok(78);
    }
    println!(
        "{}    ✓ Bitwarden CLIs  {}",
        if changed { "INSTALL" } else { "SKIP" },
        if changed {
            "updated"
        } else {
            "already installed"
        }
    );
    Ok(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validate_versions_and_digests() {
        assert!(version("1.2.3").is_some());
        assert!(version("1.2.3-dev").is_none());
        assert!(valid_digest(&"a".repeat(64)));
        assert!(!valid_digest(&"z".repeat(64)));
    }
}
