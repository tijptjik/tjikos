use crate::{compat, util::*};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    os::{
        fd::AsRawFd,
        unix::{fs::OpenOptionsExt, process::CommandExt},
    },
    path::PathBuf,
    process::{Command, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};
fn qml() -> String {
    compat::root().join("qml").to_string_lossy().into()
}
fn ipc(method: &str) -> Option<Value> {
    let args = [
        "quickshell",
        "ipc",
        "--path",
        &qml(),
        "call",
        "runtjik",
        method,
    ]
    .map(str::to_owned);
    let out = run(&args, None, 1).ok()?;
    if method == "status" {
        serde_json::from_str(&out).ok()
    } else {
        Some(json!(true))
    }
}
fn identity(pid: u64) -> Option<String> {
    let s = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<_> = s.rsplit_once(')')?.1.split_whitespace().collect();
    if fields.first() == Some(&"Z") {
        return None;
    }
    Some(fields.get(19)?.to_string())
}
fn alive(r: &Value) -> bool {
    let Some(pid) = r["pid"].as_u64() else {
        return false;
    };
    identity(pid).as_deref() == r["start"].as_str()
        && fs::read(format!("/proc/{pid}/cmdline"))
            .map(|s| s.split(|b| *b == 0).any(|s| s == qml().as_bytes()))
            .unwrap_or(false)
}
fn stop(r: &Value) -> Result<()> {
    if ipc("quit").is_none() {
        ipc("close");
    }
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(3) {
        if !alive(r) && ipc("status").is_none() {
            return Ok(());
        }
        sleep(Duration::from_millis(25));
    }
    if alive(r) {
        unsafe {
            libc::kill(r["pid"].as_u64().unwrap() as i32, libc::SIGTERM);
        }
    }
    let t = Instant::now();
    while alive(r) && t.elapsed() < Duration::from_secs(2) {
        sleep(Duration::from_millis(25));
    }
    if alive(r) || ipc("status").is_some() {
        bail!("Previous launcher did not close; refusing duplicate");
    }
    Ok(())
}
fn wait_visible(visible: bool, seconds: u64) -> Result<()> {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(seconds) {
        if let Some(v) = ipc("status") {
            if v["ready"] == true && v["visible"] == visible {
                for key in ["placementError", "clickWatchError"] {
                    if !s(&v[key]).is_empty() {
                        bail!("{}", s(&v[key]));
                    }
                }
                return Ok(());
            }
        } else if !visible {
            return Ok(());
        }
        sleep(Duration::from_millis(5));
    }
    bail!("Launcher did not finish changing visibility")
}
pub fn control(action: &str) -> Result<()> {
    let runtime = PathBuf::from(env::var("XDG_RUNTIME_DIR")?);
    let display = env::var("WAYLAND_DISPLAY")?;
    let key = format!("{:x}", Sha256::digest(format!("{}\0{display}", qml())));
    let dir = runtime.join("runtjik").join(&key[..16]);
    private_dir(&dir)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(dir.join("control.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let state = dir.join("process.json");
    let mut record: Value = fs::read(&state)
        .ok()
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or(Value::Null);
    let current = ipc("status");
    if let Some(v) = &current {
        if let Some(pid) = v["pid"].as_u64() {
            record = json!({"pid":pid,"start":identity(pid)});
        }
    }
    if action == "quit" {
        stop(&record)?;
        let _ = fs::remove_file(state);
        return Ok(());
    }
    if action == "close"
        || (action == "toggle" && current.as_ref().is_some_and(|v| v["visible"] != false))
    {
        if current.is_some() {
            ipc("close");
            return wait_visible(false, 3);
        }
        if alive(&record) {
            stop(&record)?;
            let _ = fs::remove_file(state);
        }
        return Ok(());
    }
    if current.as_ref().is_some_and(|v| v["ready"] == true) {
        if current.as_ref().unwrap()["visible"] != false {
            return Ok(());
        }
        if ipc("reopen").is_some() && wait_visible(true, 3).is_ok() {
            return Ok(());
        }
    }
    if current.is_some() || alive(&record) {
        stop(&record)?;
    }
    let _ = fs::remove_file(&state);
    let log = xdg("state").join("launcher.log");
    private_dir(log.parent().unwrap())?;
    if fs::metadata(&log).is_ok_and(|m| m.len() > 256 * 1024) {
        fs::rename(&log, log.with_extension("previous.log"))?;
    }
    let f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&log)?;
    let mut c = Command::new("quickshell");
    c.args(["--path", &qml(), "--no-duplicate"])
        .stdin(Stdio::null())
        .stdout(f.try_clone()?)
        .stderr(f);
    unsafe {
        c.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = c.spawn()?;
    let r = json!({"pid":child.id(),"start":identity(child.id() as u64)});
    atomic(&state, &serde_json::to_vec(&r)?)?;
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(10) {
        if child.try_wait()?.is_some() {
            let _ = fs::remove_file(&state);
            bail!("Launcher exited during startup; see {}", log.display());
        }
        if let Some(v) = ipc("status") {
            if v["ready"] == true {
                for key in ["placementError", "clickWatchError"] {
                    if !s(&v[key]).is_empty() {
                        stop(&r)?;
                        let _ = fs::remove_file(&state);
                        bail!("{}", s(&v[key]));
                    }
                }
                return Ok(());
            }
        }
        sleep(Duration::from_millis(25));
    }
    stop(&r)?;
    let _ = fs::remove_file(state);
    bail!("Launcher startup timed out; see {}", log.display())
}
