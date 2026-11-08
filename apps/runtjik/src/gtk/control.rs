//! A private, per-install/display/demo control socket; no idle GTK polling.
use crate::Event;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
            process::CommandExt,
        },
    },
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn socket(demo: bool) -> Result<PathBuf> {
    let runtime = PathBuf::from(std::env::var("XDG_RUNTIME_DIR")?);
    let identity = format!(
        "{}:{:?}:{demo}:{:?}",
        std::env::current_exe()?.display(),
        std::env::var_os("WAYLAND_DISPLAY"),
        std::env::var_os("RUNTJIK_SESSION")
    );
    let hash = format!("{:x}", Sha256::digest(identity));
    let directory = runtime.join("runtjik-gtk");
    fs::create_dir_all(&directory)?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    Ok(directory.join(format!("{}.sock", &hash[..16])))
}
fn request(demo: bool, request: &Value) -> Result<Value> {
    request_at(&socket(demo)?, request)
}
pub fn request_at(path: &std::path::Path, request: &Value) -> Result<Value> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    writeln!(stream, "{request}")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    let value: Value = serde_json::from_str(&line)?;
    if let Some(error) = value["error"].as_str() {
        bail!("{error}");
    }
    Ok(value)
}
/// Read-only status and idempotent quit can race a completed teardown. Do not
/// replay other commands, or conceal a live listener returning a real error.
fn request_or_gone(path: &std::path::Path, value: &Value) -> Result<Option<Value>> {
    match request_at(path, value) {
        Ok(reply) => Ok(Some(reply)),
        Err(error) if matches!(value["action"].as_str(), Some("quit" | "status")) => {
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                match UnixStream::connect(path) {
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                        ) =>
                    {
                        return Ok(None)
                    }
                    _ if Instant::now() >= deadline => return Err(error),
                    _ => std::thread::sleep(Duration::from_millis(20)),
                }
            }
        }
        Err(error) => Err(error),
    }
}
pub fn listen(demo: bool, events: async_channel::Sender<Event>) -> Result<UnixListener> {
    let path = socket(demo)?;
    // The controller holds the startup lock until this listener answers status.
    if UnixStream::connect(&path).is_ok() {
        bail!("GTK launcher is already running");
    }
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    let server = listener.try_clone()?;
    std::thread::spawn(move || {
        for incoming in server.incoming() {
            let Ok(mut stream) = incoming else { break };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
            let mut line = String::new();
            // Bound input even for malformed local clients.
            let read = std::io::Read::take(&mut stream, 16 * 1024);
            if BufReader::new(read).read_line(&mut line).is_err() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let quitting = value["action"] == "quit";
            let (send, receive) = std::sync::mpsc::sync_channel(1);
            if events.send_blocking(Event::Control(value, send)).is_err() {
                break;
            }
            if let Ok(reply) = receive.recv_timeout(Duration::from_secs(2)) {
                let _ = writeln!(stream, "{reply}");
                // Acknowledge before terminating GTK and its I/O thread.
                if quitting {
                    let _ = events.send_blocking(Event::Quit);
                }
            }
        }
    });
    Ok(listener)
}
pub fn run(action: &str, args: &[String], demo: bool) -> Result<()> {
    anyhow::ensure!(
        [
            "show",
            "toggle",
            "close",
            "close-target",
            "quit",
            "status",
            "test-query",
            "test-response",
            "test-activate",
            "test-request",
            "outside-click",
            "--demo"
        ]
        .contains(&action),
        "Unknown GTK action: {action}"
    );
    let action = if action == "--demo" { "show" } else { action };
    let path = socket(demo)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path.with_extension("lock"))?;
    anyhow::ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } == 0,
        "Cannot lock GTK startup"
    );
    let command = json!({"action":action, "text":args.first().filter(|s| s.as_str() != "--demo").cloned().unwrap_or_default()});
    // A live socket returning an error must not be treated as permission to spawn.
    if UnixStream::connect(&path).is_ok() {
        if let Some(reply) = request_or_gone(&path, &command)? {
            if action == "close-target" {
                anyhow::ensure!(
                    reply["handled"].is_boolean(),
                    "Missing close-target decision"
                );
                println!("{}", reply["handled"]);
            }
            if action == "status" {
                println!("{reply}");
            }
            return Ok(());
        }
    }
    let recovery = crate::recovery::path(demo)?;
    if UnixStream::connect(&recovery).is_ok() {
        if let Some(reply) = request_or_gone(&recovery, &command)? {
            if action == "close-target" {
                anyhow::ensure!(
                    reply["handled"].is_boolean(),
                    "Missing close-target decision"
                );
                println!("{}", reply["handled"]);
            }
            if action == "status" {
                println!("{reply}");
            }
            return Ok(());
        }
    }
    if action == "close-target" {
        println!("false");
        return Ok(());
    }
    if ["close", "quit"].contains(&action) {
        return Ok(());
    }
    if action == "status" {
        println!(
            "{}",
            json!({"running":false,"ready":false,"visible":false,"renderer":"gtk4"})
        );
        return Ok(());
    }
    anyhow::ensure!(
        ["show", "toggle"].contains(&action),
        "GTK launcher is not running"
    );
    let log = path.with_extension("log");
    if fs::metadata(&log).is_ok_and(|m| m.len() > 256 * 1024) {
        fs::rename(&log, log.with_extension("previous.log"))?;
    }
    let output = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&log)?;
    let mut child = Command::new(std::env::current_exe()?);
    child.arg("serve");
    if demo {
        child.arg("--demo");
    }
    child
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output);
    unsafe {
        child.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = child.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait()? {
            bail!("GTK launcher exited ({status}); see {}", log.display());
        }
        if let Ok(status) = request(demo, &json!({"action":"status"})) {
            if status["ready"] == true {
                return Ok(());
            }
            if let Some(error) = status["backend_error"].as_str().filter(|s| !s.is_empty()) {
                bail!("{error}");
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    child
        .kill()
        .context("Stopping GTK launcher after startup timeout")?;
    child.wait()?;
    bail!("GTK launcher did not become ready; see {}", log.display())
}
