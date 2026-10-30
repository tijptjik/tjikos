use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::{
    env, fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub fn home() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}
pub fn state() -> PathBuf {
    env::var_os("NOTJIK_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".local/state"))
                .join("notjik")
        })
}
pub fn config_path() -> PathBuf {
    env::var_os("NOTJIK_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".config"))
                .join("notjik/config.json")
        })
}
pub fn telegram_dir() -> PathBuf {
    env::var_os("NOTJIK_TELEGRAM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| state().join("telegram"))
}
pub fn private_dir(p: &Path) -> Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(p)?;
    let m = fs::symlink_metadata(p)?;
    if !m.is_dir() || m.is_symlink() {
        bail!("Private directory must not be a symlink")
    }
    fs::set_permissions(p, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub fn private_write(p: &Path, bytes: &[u8]) -> Result<()> {
    let dir = p.parent().context("Missing parent")?;
    private_dir(dir)?;
    let mut f = tempfile::NamedTempFile::new_in(dir)?;
    f.write_all(bytes)?;
    f.as_file().sync_all()?;
    f.persist(p)?;
    Ok(())
}
pub fn read_json(p: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
pub fn uri(p: &Path) -> Result<String> {
    Ok(url::Url::from_file_path(p)
        .map_err(|_| anyhow::anyhow!("Invalid file path"))?
        .into())
}
pub fn expand(p: &str) -> PathBuf {
    if p == "~" {
        home()
    } else if let Some(s) = p.strip_prefix("~/") {
        home().join(s)
    } else {
        PathBuf::from(p)
    }
}
pub fn emit(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}
pub fn run(cmd: &mut Command, timeout: Duration) -> Result<Vec<u8>> {
    run_cancelled(cmd, timeout, None)
}
pub fn run_cancelled(
    cmd: &mut Command,
    timeout: Duration,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<Vec<u8>> {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let a = std::thread::spawn(move || {
        let mut b = Vec::new();
        out.read_to_end(&mut b).map(|_| b)
    });
    let b = std::thread::spawn(move || {
        let mut b = Vec::new();
        err.read_to_end(&mut b).map(|_| b)
    });
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if start.elapsed() > timeout
            || cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed))
        {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            let _ = a.join();
            let _ = b.join();
            bail!("Command cancelled or timed out")
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let out = a
        .join()
        .map_err(|_| anyhow::anyhow!("Output reader failed"))??;
    let err = b
        .join()
        .map_err(|_| anyhow::anyhow!("Error reader failed"))??;
    if !status.success() {
        bail!("{}", String::from_utf8_lossy(&err).trim())
    }
    Ok(out)
}
pub fn ipc(root: &Path, request: &Value) -> Result<Value> {
    let bytes = serde_json::to_vec(request)?;
    if bytes.len() > 131072 {
        bail!("Request exceeds 128 KiB")
    }
    let dir = if let Some(d) = env::var_os("XDG_RUNTIME_DIR") {
        tempfile::Builder::new()
            .prefix("notjik-ipc-")
            .tempdir_in(d)?
    } else {
        tempfile::Builder::new().prefix("notjik-ipc-").tempdir()?
    };
    let p = dir.path().join("request.json");
    private_write(&p, &bytes)?;
    let out = run(
        Command::new("quickshell")
            .args(["ipc", "--path"])
            .arg(root.join("qml"))
            .args(["call", "notjik", "requestFile"])
            .arg(&p),
        Duration::from_secs(5),
    )?;
    let response: Value =
        serde_json::from_slice(&out).context("Unexpected Quickshell IPC response")?;
    if response["ok"] != true {
        bail!("{}", response["error"].as_str().unwrap_or("request failed"))
    }
    Ok(response)
}
pub fn merge(base: &mut Value, patch: &Value) {
    if let (Some(b), Some(p)) = (base.as_object_mut(), patch.as_object()) {
        for (k, v) in p {
            if let Some(old) = b.get_mut(k) {
                merge(old, v)
            } else {
                b.insert(k.clone(), v.clone());
            }
        }
    } else {
        *base = patch.clone();
    }
}
/// JSON truthiness at the QML/TDLib boundary, matching the previous worker.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|v| v != 0.),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}
