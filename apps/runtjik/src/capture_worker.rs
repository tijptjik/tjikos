use crate::{capture, util::*};
use anyhow::{bail, ensure, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::OpenOptionsExt,
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}
fn lua_string(value: &str) -> String {
    let mut equals = String::new();
    while value.contains(&format!("]{equals}]")) {
        equals.push('=');
    }
    format!("[{equals}[{value}]{equals}]")
}
struct StopShortcut(u32);
impl StopShortcut {
    fn install() -> Result<Option<Self>> {
        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
            return Ok(None);
        }
        let executable = std::env::current_exe()?;
        let command = format!(
            "'{}' stop-recording",
            executable.to_string_lossy().replace('\'', "'\\''")
        );
        let token = std::process::id();
        let script = format!(
            "local stop_command = {}; local stop_token = {token};\n{}",
            lua_string(&command),
            include_str!("recording_stop.lua")
        );
        command_eval(&script)?;
        Ok(Some(Self(token)))
    }
}
fn command_eval(script: &str) -> Result<String> {
    run(&["hyprctl".into(), "eval".into(), script.into()], None, 2)
}
impl Drop for StopShortcut {
    fn drop(&mut self) {
        let _ = command_eval(&format!(
            "if _G.runtjikRecordingStop and _G.runtjikRecordingStop.token == {} then \
             for _, binding in ipairs(_G.runtjikRecordingStop.bindings) do binding:remove() end; \
             _G.runtjikRecordingStop = nil end",
            self.0
        ));
    }
}
pub fn socket() -> Result<PathBuf> {
    let root = PathBuf::from(
        std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() })),
    )
    .join("runtjik-capture");
    private_dir(&root)?;
    Ok(root.join("recorder.sock"))
}
pub fn request(action: &str) -> Result<Option<Value>> {
    request_at(&socket()?, action)
}
fn request_at(path: &Path, action: &str) -> Result<Option<Value>> {
    let mut stream = match UnixStream::connect(path) {
        Ok(s) => s,
        Err(e)
            if [
                std::io::ErrorKind::NotFound,
                std::io::ErrorKind::ConnectionRefused,
            ]
            .contains(&e.kind()) =>
        {
            return Ok(None)
        }
        Err(e) => return Err(e.into()),
    };
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    if let Err(error) = writeln!(stream, "{action}") {
        if recorder_disconnected(&error) {
            return Ok(None);
        }
        return Err(error.into());
    }
    let mut data = Vec::new();
    if let Err(error) = stream.take(8193).read_to_end(&mut data) {
        if recorder_disconnected(&error) {
            return Ok(None);
        }
        return Err(error.into());
    }
    // The encoder can finish after connect but before the worker accepts us.
    if data.is_empty() {
        return Ok(None);
    }
    ensure!(data.len() <= 8192, "Invalid recorder response");
    Ok(Some(serde_json::from_slice(&data)?))
}

#[cfg(test)]
mod request_tests {
    use super::*;

    #[test]
    fn worker_finishing_during_a_request_is_no_longer_active() {
        for read_request in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("recorder.sock");
            let listener = UnixListener::bind(&path).unwrap();
            let worker = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                if read_request {
                    let mut bytes = [0; 64];
                    stream.read(&mut bytes).unwrap();
                }
                // Exit without replying, as when the encoder finishes between
                // a client's connect and the worker's next loop iteration.
            });
            assert!(request_at(&path, "status").unwrap().is_none());
            worker.join().unwrap();
        }
    }
}
fn recorder_disconnected(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::UnexpectedEof
    )
}
fn argv(spec: &Value) -> Result<Vec<String>> {
    if spec["mode"] == "window" {
        let mut a: Vec<String> = [
            "gpu-screen-recorder",
            "-w",
            "portal",
            "-restore-portal-session",
            "no",
            "-f",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        a.push(spec["fps"].to_string());
        a.extend(
            ["-k", "h264", "-fallback-cpu-encoding", "yes"]
                .into_iter()
                .map(str::to_owned),
        );
        if spec["audio"] == true {
            a.extend(
                ["-ac", "aac", "-a", "default_output"]
                    .into_iter()
                    .map(str::to_owned),
            );
        }
        a.extend(["-o".into(), s(&spec["file"]).into()]);
        return Ok(a);
    }
    let mut a = vec![
        "wf-recorder".into(),
        "--no-dmabuf".into(),
        "-r".into(),
        spec["fps"].to_string(),
    ];
    if spec["audio"] == true {
        let sink = command(&["pactl", "get-default-sink"])?;
        ensure!(
            !sink.trim().is_empty(),
            "No system audio output is available"
        );
        a.push(format!("--audio={}.monitor", sink.trim()));
    }
    for (k, flag) in [("output", "-o"), ("area", "-g")] {
        if let Some(v) = spec[k].as_str().filter(|s| !s.is_empty()) {
            a.extend([flag.into(), v.into()]);
        }
    }
    a.extend(["-f".into(), s(&spec["file"]).into()]);
    Ok(a)
}
pub fn serve(spec: Value) -> Result<()> {
    unsafe {
        libc::umask(0o077);
        libc::signal(libc::SIGTERM, stop as *const () as usize);
        libc::signal(libc::SIGINT, stop as *const () as usize);
    }
    let path = socket()?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path.with_extension("lock"))?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "A Runtjik recording is already active"
    );
    let _ = fs::remove_file(&path);
    let server = UnixListener::bind(&path)?;
    server.set_nonblocking(true)?;
    let result = (|| -> Result<()> {
        let logdir = xdg("state");
        private_dir(&logdir)?;
        let log = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(logdir.join("capture.log"))?;
        let a = argv(&spec)?;
        let _shortcut = StopShortcut::install()?;
        let mut child = Command::new(&a[0])
            .args(&a[1..])
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?;
        let mut stopping = None;
        let mut workspace_check = Instant::now();
        let outcome = (|| -> Result<()> {
            std::thread::sleep(Duration::from_millis(300));
            ensure!(
                child.try_wait()?.is_none(),
                "Recorder exited during startup; inspect {}",
                logdir.join("capture.log").display()
            );
            println!("{}", json!({"ready":true}));
            std::io::stdout().flush()?;
            while child.try_wait()?.is_none() {
                if let Ok((mut connection, _)) = server.accept() {
                    let _ = connection.set_read_timeout(Some(Duration::from_millis(500)));
                    let _ = connection.set_write_timeout(Some(Duration::from_millis(500)));
                    let mut bytes = [0; 64];
                    if let Ok(n) = connection.read(&mut bytes) {
                        if String::from_utf8_lossy(&bytes[..n]).trim() == "stop" {
                            STOP.store(true, Ordering::Relaxed);
                        }
                        let response = json!({"file":spec["file"],"mode":spec["mode"],"stopping":STOP.load(Ordering::Relaxed)}).to_string();
                        let _ = connection.write_all(response.as_bytes());
                    }
                }
                if spec["mode"] == "workspace"
                    && !STOP.load(Ordering::Relaxed)
                    && workspace_check.elapsed() >= Duration::from_millis(100)
                {
                    workspace_check = Instant::now();
                    let same = capture::hypr("monitors")
                        .ok()
                        .and_then(|ms| ms.into_iter().find(|m| m["name"] == spec["output"]))
                        .is_some_and(|m| capture::workspace(&m)["id"] == spec["workspace"]);
                    if !same {
                        STOP.store(true, Ordering::Relaxed);
                    }
                }
                if STOP.load(Ordering::Relaxed) && stopping.is_none() {
                    unsafe {
                        libc::kill(child.id() as i32, libc::SIGINT);
                    }
                    stopping = Some(Instant::now());
                }
                if stopping.is_some_and(|t| t.elapsed() > Duration::from_secs(20)) {
                    child.kill()?;
                    child.wait()?;
                    bail!("Recorder did not finalize within 20 seconds; output may be incomplete");
                }
                // Wake immediately for Stop/status requests instead of making
                // every caller wait for the workspace polling interval.
                let mut event = libc::pollfd {
                    fd: server.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                unsafe {
                    libc::poll(&mut event, 1, 100);
                }
            }
            let status = child.wait()?;
            // Finalization is complete. Do not leave a socket that cannot answer
            // status requests while the notification command is running.
            let _ = fs::remove_file(&path);
            use std::os::unix::process::ExitStatusExt;
            let success = (status.success()
                || status.code() == Some(130)
                || status.signal() == Some(libc::SIGINT))
                && fs::metadata(s(&spec["file"])).is_ok_and(|m| m.len() > 0);
            let message = if success {
                format!("Saved {}", s(&spec["file"]))
            } else {
                format!(
                    "Recording failed; inspect {}",
                    logdir.join("capture.log").display()
                )
            };
            let _ = command(&[
                "notify-send",
                "--app-name=Runtjik",
                "Screen recording",
                &message,
            ]);
            Ok(())
        })();
        if child.try_wait()?.is_none() {
            unsafe {
                libc::kill(child.id() as i32, libc::SIGINT);
            }
            let deadline = Instant::now() + Duration::from_secs(20);
            while child.try_wait()?.is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
            child.wait()?;
        }
        outcome
    })();
    let _ = fs::remove_file(path);
    result
}
