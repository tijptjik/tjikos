//! A non-GTK supervisor survives the display connection. State stays in memory.
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tjik_desktop::DesktopState;
pub fn path(demo: bool) -> Result<std::path::PathBuf> {
    Ok(crate::control::socket(demo)?.with_extension("recovery.sock"))
}
pub fn remember(value: Value) {
    let Some(path) = std::env::var_os("RUNTJIK_RECOVERY_SOCKET") else {
        return;
    };
    static LATEST: Mutex<Option<Value>> = Mutex::new(None);
    static WAKE: OnceLock<std::sync::mpsc::SyncSender<()>> = OnceLock::new();
    *LATEST.lock().unwrap() = Some(value);
    let tx = WAKE.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                let value = LATEST.lock().unwrap().take();
                if let (Some(value), Ok(mut stream)) = (value, UnixStream::connect(&path)) {
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
                    let _ = writeln!(stream, "{}", json!({"state":value}));
                }
            }
        });
        tx
    });
    let _ = tx.try_send(());
}
struct Cleanup(std::path::PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
pub fn run(demo: bool) -> Result<()> {
    let path = path(demo)?;
    if UnixStream::connect(&path).is_ok() {
        bail!("Launcher recovery supervisor already running");
    }
    let _ = fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let _cleanup = Cleanup(path.clone());
    let executable = std::env::current_exe()?;
    let desktop = tjik_desktop::selected();
    let subscription = desktop.subscribe().ok();
    let mut generation = desktop.snapshot().ok().map(|s| s.generation);
    let mut state = json!({"visible":true,"query":"","pinned":false,"view":"grid"});
    let mut restored = false;
    let mut child: Option<std::process::Child> = None;
    let mut lost = false;
    let mut exited = None;
    let mut child_generation = generation.clone();
    loop {
        for incoming in listener.incoming().take(16) {
            let mut stream = match incoming {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            };
            stream.set_read_timeout(Some(Duration::from_millis(100)))?;
            let mut line = String::new();
            let mut reader = BufReader::new(std::io::Read::take(&mut stream, 65536));
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if let Some(value) = message.get("state") {
                state = value.clone();
                continue;
            }
            match message["action"].as_str().unwrap_or("") {
                "quit" => {
                    if let Some(mut c) = child.take() {
                        let _: std::io::Result<()> = c.kill();
                        let _ = c.wait();
                    }
                    let _ = writeln!(stream, "{{\"ok\":true}}");
                    return Ok(());
                }
                "close" => state["visible"] = json!(false),
                "show" | "toggle" => state["visible"] = json!(true),
                "status" => (),
                _ => {
                    let _ = writeln!(stream, "{{\"error\":\"Launcher is recovering\"}}");
                    continue;
                }
            }
            let _ = writeln!(
                stream,
                "{}",
                json!({"running":true,"recovering":true,"ready":false,"visible":false,"renderer":"gtk4","supervisor_pid":std::process::id()})
            );
        }
        if let Some(subscription) = &subscription {
            if let Ok(Some(event)) = subscription.recv_timeout(Duration::from_millis(100)) {
                match event.state {
                    DesktopState::Unavailable(_) => {
                        lost = true;
                        generation = None;
                    }
                    DesktopState::Ready(snapshot) => generation = Some(snapshot.generation),
                }
            }
        } else {
            std::thread::sleep(Duration::from_millis(100));
        }
        if let Some(c) = child.as_mut() {
            if let Some(status) = c.try_wait()? {
                eprintln!("Desktop recovery: surface exited {status}");
                child = None;
                if status.success() {
                    return Ok(());
                }
                exited = Some(Instant::now());
            }
        }
        if child.is_none() {
            if exited.is_some() && !lost && generation == child_generation {
                if exited.is_some_and(|t| t.elapsed() > Duration::from_secs(3)) {
                    bail!(
                        "GTK exited without a compositor disconnect; not restarting a crashed app"
                    );
                }
                continue;
            }
            if restored && generation.is_none() {
                continue;
            }
            let mut command = Command::new(&executable);
            command.arg("surface").env("RUNTJIK_RECOVERY_SOCKET", &path);
            if demo {
                command.arg("--demo");
            }
            if restored {
                command.env("RUNTJIK_RESTORE", serde_json::to_string(&state)?);
            }
            command.envs(tjik_desktop::environment());
            command.stdin(Stdio::null());
            unsafe {
                command.pre_exec(|| {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let process = command.spawn()?;
            eprintln!(
                "Desktop recovery: started surface {} (restored={restored})",
                process.id()
            );
            child = Some(process);
            child_generation = generation.clone();
            restored = true;
            lost = false;
            exited = None;
        }
    }
}
