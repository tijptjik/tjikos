use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::fd::AsRawFd,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::Command,
};
fn expand(s: &str) -> PathBuf {
    s.strip_prefix("~/")
        .map(|r| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(r))
        .unwrap_or_else(|| s.into())
}
fn state_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("XDG_STATE_HOME").unwrap_or_else(|| expand("~/.local/state").into()),
    )
    .join("tjikup")
}
fn lock(path: &Path) -> Result<fs::File> {
    let f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
        bail!("cannot acquire workflow lock");
    }
    Ok(f)
}
fn save(path: &Path, value: &Value) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
fn systemctl(args: &[&str]) -> Result<String> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()?;
    let text = String::from_utf8(output.stdout)?;
    if !output.status.success()
        && !(args.first() == Some(&"show") && text.contains("LoadState=not-found"))
    {
        bail!(
            "systemctl {} failed; inspect the unit journal",
            args.first().unwrap_or(&"query")
        );
    }
    Ok(text)
}
fn properties(unit: &str) -> Result<BTreeMap<String, String>> {
    Ok(systemctl(&[
        "show",
        unit,
        "--property=LoadState,ActiveState,Type,ExecStart,NeedDaemonReload",
    ])?
    .lines()
    .filter_map(|l| l.split_once('=').map(|(k, v)| (k.into(), v.into())))
    .collect())
}
fn paths(pattern: &str) -> Result<Vec<PathBuf>> {
    let pattern = std::ffi::CString::new(pattern)?;
    let mut glob: libc::glob_t = unsafe { std::mem::zeroed() };
    let status = unsafe { libc::glob(pattern.as_ptr(), 0, None, &mut glob) };
    let mut result = Vec::new();
    if status == 0 {
        for i in 0..glob.gl_pathc {
            use std::os::unix::ffi::OsStrExt;
            let bytes = unsafe { std::ffi::CStr::from_ptr(*glob.gl_pathv.add(i)) }.to_bytes();
            result.push(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)));
        }
    }
    unsafe { libc::globfree(&mut glob) };
    if status != 0 && status != libc::GLOB_NOMATCH {
        bail!("failed to expand service input");
    }
    result.sort();
    Ok(result)
}
fn visit(
    path: &Path,
    digest: &mut Sha256,
    source: bool,
    service: bool,
    ancestors: &mut Vec<PathBuf>,
) -> Result<()> {
    digest.update(path.as_os_str().as_encoded_bytes());
    let meta = match path.symlink_metadata() {
        Ok(m) => m,
        Err(e) if service && e.kind() == std::io::ErrorKind::NotFound => {
            digest.update(b"missing");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    if meta.is_symlink() && service {
        digest.update(fs::read_link(path)?.as_os_str().as_encoded_bytes());
        let target = path.canonicalize()?;
        if ancestors.contains(&target) {
            bail!("symlink cycle in service inputs");
        }
        ancestors.push(target.clone());
        visit(&target, digest, source, service, ancestors)?;
        ancestors.pop();
        return Ok(());
    }
    digest.update((meta.mode() & 0o7777).to_string().as_bytes());
    if meta.is_symlink() {
        digest.update(b"link");
        digest.update(fs::read_link(path)?.as_os_str().as_encoded_bytes());
        if !source {
            digest.update(fs::read(path)?);
        }
    } else if meta.is_dir() {
        let mut children = fs::read_dir(path)?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        children.sort();
        for child in children {
            let name = child.file_name().unwrap().to_string_lossy();
            if name == "__pycache__"
                || service && [".git", ".pytest_cache"].contains(&name.as_ref())
                || !service
                    && (name.ends_with(".pyc")
                        || name.ends_with(".pyo")
                        || source
                            && [".git", "dist", "target", ".venv", "node_modules"]
                                .contains(&name.as_ref()))
            {
                continue;
            }
            visit(&child, digest, source, service, ancestors)?;
        }
    } else if meta.is_file() {
        use std::io::Read;
        let mut f = fs::File::open(path)?;
        let mut buf = [0; 65536];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            digest.update(&buf[..n]);
        }
    } else {
        bail!("non-file workflow input");
    }
    Ok(())
}
fn fingerprint(unit: &str, inputs: &[String], info: &BTreeMap<String, String>) -> Result<String> {
    let mut digest = Sha256::new();
    digest.update(systemctl(&["cat", unit])?.as_bytes());
    let mut patterns = inputs.to_vec();
    for token in info
        .get("ExecStart")
        .map(String::as_str)
        .unwrap_or_default()
        .split([' ', '{', ';', '}'])
    {
        if let Some(p) = token.strip_prefix("path=") {
            if !p.is_empty() {
                patterns.push(p.into());
            }
        }
    }
    patterns.sort();
    patterns.dedup();
    for p in patterns {
        let p = expand(&p);
        digest.update(p.as_os_str().as_encoded_bytes());
        let mut matches = paths(&p.to_string_lossy())?;
        if matches.is_empty() {
            matches.push(p);
        }
        for path in matches {
            visit(&path, &mut digest, false, true, &mut Vec::new())?;
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}
pub fn refresh(repo: &Path, args: &[String]) -> Result<i32> {
    let mut units: BTreeMap<String, Vec<String>> =
        serde_json::from_slice(&fs::read(repo.join("scripts/user-service-inputs.json"))?)?;
    for entry in fs::read_dir(repo.join("native/home/files/.config/systemd/user"))? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.contains(".timer") {
            units.insert(name.trim_end_matches(".j2").into(), vec![]);
        }
    }
    let mut check = false;
    let mut selected = Vec::new();
    let mut excluded = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" => check = true,
            "--unit" => selected.push(args.next().context("--unit needs name")?.clone()),
            "--exclude-unit" => {
                excluded.push(args.next().context("--exclude-unit needs name")?.clone())
            }
            _ => bail!("unknown service option"),
        }
    }
    if selected.iter().any(|u| !units.contains_key(u)) {
        bail!("unknown managed service");
    }
    units.retain(|u, _| (selected.is_empty() || selected.contains(u)) && !excluded.contains(u));
    let directory = state_dir();
    let state = directory.join("user-services.json");
    let _lock = if !check {
        fs::create_dir_all(&directory)?;
        Some(lock(&directory.join("user-services.lock"))?)
    } else {
        None
    };
    let mut saved: Value = fs::read(&state)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .filter(Value::is_object)
        .unwrap_or(json!({}));
    println!(
        "__tjikup_section__:{}",
        std::env::var("TJIKUP_SERVICE_SECTION").unwrap_or_else(|_| "Services".into())
    );
    let hide = std::env::var_os("TJIKUP_HIDE_UNCHANGED_SERVICES").is_some();
    let mut pending = 0;
    let mut reloaded = false;
    for (unit, inputs) in units {
        let mut info = properties(&unit)?;
        if !check && info.get("NeedDaemonReload").is_some_and(|v| v == "yes") {
            systemctl(&["daemon-reload"])?;
            reloaded = true;
            info = properties(&unit)?;
        }
        if info.get("LoadState").map(String::as_str) != Some("loaded")
            || info.get("ActiveState").map(String::as_str) != Some("active")
        {
            if !hide {
                println!("SKIP    - {unit}  inactive or unavailable");
            }
            continue;
        }
        if unit.ends_with(".service") && info.get("Type").is_some_and(|v| v == "oneshot") {
            bail!("refusing to restart job {unit}");
        }
        let current = fingerprint(&unit, &inputs, &info)?;
        if saved[&unit] == current {
            if !hide {
                println!("SKIP    ✓ {unit}  runtime unchanged");
            }
            continue;
        }
        pending += 1;
        if check {
            println!("CHECK   ✓ {unit}  would refresh");
            continue;
        }
        systemctl(&["try-restart", &unit])?;
        let after = properties(&unit)?;
        if after.get("ActiveState").map(String::as_str) != Some("active") {
            bail!("{unit} did not become active after refresh");
        }
        if fingerprint(&unit, &inputs, &after)? != current {
            bail!("{unit} inputs changed during refresh; retry");
        }
        saved[&unit] = json!(current);
        save(&state, &saved)?;
        println!("RESTART ✓ {unit}  updated runtime active");
    }
    if pending == 0 {
        println!(
            "__stage_verb__:{}\n__stage_note__:{}",
            if reloaded { "CONFIG" } else { "SKIP" },
            if reloaded {
                "unit definitions reloaded"
            } else {
                "no runtime changes"
            }
        );
    } else if !check {
        println!("__stage_note__:{pending} active units refreshed");
    }
    Ok(0)
}
pub fn cache(args: &[String]) -> Result<i32> {
    let name = args.first().context("cache-install needs name")?;
    if ![
        "tjikup", "notjik", "bartjik", "greetjik", "runtjik", "tasktjik",
    ]
    .contains(&name.as_str())
    {
        bail!("unknown install cache");
    }
    let separator = args
        .iter()
        .position(|v| v == "--")
        .context("cache-install needs -- COMMAND")?;
    let command = &args[separator + 1..];
    if command.is_empty() {
        bail!("missing install command");
    }
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    let mut options = args[1..separator].iter();
    while let Some(key) = options.next() {
        let path = PathBuf::from(options.next().context("missing cache path")?);
        match key.as_str() {
            "--input" => inputs.push(path),
            "--output" => outputs.push(path),
            _ => bail!("unknown cache option"),
        }
    }
    if inputs.is_empty() || outputs.is_empty() {
        bail!("cache requires inputs and outputs");
    }
    let fingerprint = |paths: &[PathBuf], source: bool| -> Option<String> {
        let mut d = Sha256::new();
        for p in paths {
            visit(p, &mut d, source, false, &mut Vec::new()).ok()?;
        }
        Some(format!("{:x}", d.finalize()))
    };
    let dir = state_dir().join("install");
    fs::create_dir_all(&dir)?;
    let _lock = lock(&dir.join(format!("{name}.lock")))?;
    let record = dir.join(format!("{name}.json"));
    let before = fingerprint(&inputs, true);
    let deployed = fingerprint(&outputs, false);
    let installed = outputs.iter().any(|p| p.symlink_metadata().is_ok());
    let report = |verb: &str, note: &str| {
        println!("{verb:<7} ✓ {name}    {note}\n__stage_verb__:{verb}\n__stage_note__:{note}")
    };
    let saved: Value = fs::read(&record)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null);
    if before.is_some()
        && deployed.is_some()
        && saved == json!({"inputs":before,"outputs":deployed})
    {
        report("SKIP", "no changes");
        return Ok(0);
    }
    let result = Command::new(&command[0]).args(&command[1..]).status()?;
    if !result.success() {
        return Ok(result.code().unwrap_or(1));
    }
    let after = fingerprint(&outputs, false);
    if before.is_some() && after.is_some() && before == fingerprint(&inputs, true) {
        save(&record, &json!({"inputs":before,"outputs":after}))?;
    }
    if deployed.is_some() && after == deployed {
        report("SKIP", "no changes");
    } else if installed {
        report(
            "UPDATE",
            if deployed.is_some() {
                "updated"
            } else {
                "repaired"
            },
        );
    } else {
        report("INSTALL", "installed");
    }
    Ok(0)
}
pub fn activate(repo: &Path, args: &[String]) -> Result<i32> {
    if args.len() != 1 || !["bartjik", "notjik"].contains(&args[0].as_str()) {
        bail!("activate requires bartjik or notjik");
    }
    let app = &args[0];
    let script = repo.join(format!("scripts/activate-{app}.sh"));
    let checked = Command::new("bash")
        .arg(&script)
        .arg("--check")
        .output()?
        .status
        .code()
        .unwrap_or(2);
    if ![0, 1].contains(&checked) {
        return Ok(checked);
    }
    if checked == 1 && !Command::new("bash").arg(&script).output()?.status.success() {
        bail!("{app} activation failed");
    }
    refresh(repo, &["--unit".into(), format!("{app}.service")])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_tracks_target_bytes() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("input");
        fs::write(&file, b"a").unwrap();
        let mut a = Sha256::new();
        visit(&file, &mut a, true, false, &mut vec![]).unwrap();
        fs::write(&file, b"b").unwrap();
        let mut b = Sha256::new();
        visit(&file, &mut b, true, false, &mut vec![]).unwrap();
        assert_ne!(a.finalize(), b.finalize());
    }
}
