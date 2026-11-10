use crate::{capture, util::*};
use anyhow::{bail, ensure, Result};
use serde_json::{json, Value};
use std::{
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io::{BufRead, BufReader, Read, Write},
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

fn recording_actions(file: &str) -> Value {
    json!([
        {"label":"View video", "identifier":"default", "command":[crate::paths::root().join("runtjik-core"), "view-recording", file]},
        {"label":"Share on Telegram", "identifier":"telegram", "command":[crate::paths::root().join("runtjik-core"), "share-recording-telegram", file]}
    ])
}

pub fn view_recording(file: &str) -> Result<()> {
    ensure!(Path::new(file).is_file(), "Recording no longer exists");
    // Match the desktop launcher's Qt interface, and keep this recording's
    // player separate from any existing VLC session or playback.
    let mut command = if let Some(qvlc) = which("qvlc") {
        Command::new(qvlc)
    } else {
        let mut command = Command::new("vlc");
        command.args(["--intf", "qt"]);
        command
    };
    let mut child = command
        .args(["--no-one-instance", "--", file])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let pid = child.id() as u64;
    focus_recording_window(&mut child, "VLC", |client| {
        client["pid"].as_u64() == Some(pid)
    })
}

pub fn share_recording_telegram(file: &str) -> Result<()> {
    ensure!(Path::new(file).is_file(), "Recording no longer exists");
    let telegram = which("telegram-desktop")
        .or_else(|| which("Telegram"))
        .or_else(|| which("/opt/Telegram/Telegram"))
        .unwrap_or_else(|| PathBuf::from("telegram-desktop"));
    let mut child = Command::new(telegram)
        .args(["-sendpath", file])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    focus_recording_window(&mut child, "Telegram", |client| {
        let class = s(&client["app_id"]).to_ascii_lowercase();
        (matches!(
            class.as_str(),
            "telegram" | "telegram-desktop" | "telegramdesktop" | "org.telegram.desktop"
        ) || class.starts_with("org.telegram.desktop."))
            && !s(&client["title"]).contains("Media viewer")
    })
}

fn focus_recording_window(
    child: &mut std::process::Child,
    app: &str,
    matches: impl Fn(&Value) -> bool,
) -> Result<()> {
    if !tjik_desktop::selected().diagnostics().capabilities.activate {
        return Ok(());
    }
    // Wait for cold starts without handing focus to an unrelated existing player.
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(100));
        if let Some(status) = child.try_wait()? {
            ensure!(
                status.success(),
                "{app} exited before opening its window ({status})"
            );
        }
        let clients = crate::windows::clients()?;
        let target = clients
            .iter()
            .filter(|client| client["mapped"] != false && matches(client))
            .min_by_key(|client| {
                client["focus_order"]
                    .as_i64()
                    .filter(|id| *id >= 0)
                    .unwrap_or(i64::MAX)
            });
        if let Some(target) = target {
            tjik_desktop::selected().activate_newly_mapped(&crate::windows::id(target)?)?;
            return Ok(());
        }
    }
    bail!("{app} did not expose a window to focus")
}

fn recording_duration(file: &str) -> Option<String> {
    let raw = run(
        &[
            "ffprobe".into(),
            "-v".into(),
            "error".into(),
            "-show_entries".into(),
            "format=duration".into(),
            "-of".into(),
            "default=noprint_wrappers=1:nokey=1".into(),
            "-i".into(),
            file.into(),
        ],
        None,
        3,
    )
    .ok()?;
    let seconds = raw.trim().parse::<f64>().ok()?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    let seconds = seconds.round().max(1.0) as u64;
    Some(if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    })
}

fn recording_artwork(file: &str) -> Option<PathBuf> {
    let root = std::env::var_os("NOTJIK_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/lib/notjik"));
    let mut images: Vec<_> = fs::read_dir(root.join("assets/icons/bonsai"))
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path.extension().is_some_and(|ext| ext == "png")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("bonsai-"))
        })
        .collect();
    images.sort();
    if images.is_empty() {
        return None;
    }
    let mut hash = DefaultHasher::new();
    file.hash(&mut hash);
    Some(images[hash.finish() as usize % images.len()].clone())
}

fn notify_finished(file: &str, success: bool, logdir: &Path) {
    let message = if success {
        recording_duration(file)
            .map(|duration| format!("Duration: {duration}"))
            .unwrap_or_else(|| "Duration unavailable".into())
    } else {
        format!(
            "Recording failed; inspect {}",
            logdir.join("capture.log").display()
        )
    };
    if success && which("notjikctl").is_some() {
        // Persist command actions in Notjik; never wait for a click while owning
        // the recorder lock, socket or temporary Super bindings.
        let actions = recording_actions(file).to_string();
        let artwork = recording_artwork(file);
        let mut args = vec![
            "notjikctl",
            "send",
            "Screen recording",
            &message,
            "--app",
            "Runtjik",
            "--type",
            "status",
            "--data",
            r#"{"state":"Recording complete","tone":"success"}"#,
            "--timeout",
            "never",
            "--actions",
            &actions,
        ];
        let image = artwork.as_ref().map(|path| path.to_string_lossy());
        if let Some(image) = &image {
            args.extend(["--image", image.as_ref()]);
        }
        if command(&args).is_ok() {
            return;
        }
    }
    let _ = command(&[
        "notify-send",
        "--app-name=Runtjik",
        "Screen recording",
        &message,
    ]);
}

extern "C" fn stop(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}
struct StopShortcut(u32);
impl StopShortcut {
    fn install() -> Result<Option<Self>> {
        if !tjik_desktop::selected()
            .diagnostics()
            .capabilities
            .recording_shortcut
        {
            return Ok(None);
        }
        let executable = std::env::current_exe()?;
        let command = format!(
            "{} stop-recording",
            shlex::try_quote(&executable.to_string_lossy())?
        );
        let token = std::process::id();
        tjik_desktop::selected().recording_shortcut(token, Some(&command))?;
        Ok(Some(Self(token)))
    }
}
impl Drop for StopShortcut {
    fn drop(&mut self) {
        let _ = tjik_desktop::selected().recording_shortcut(self.0, None);
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

fn encoder_request(path: &Path, name: &str, timeout: Duration) -> Result<Value> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    writeln!(stream, "{}", json!({"id":1,"name":name}))?;
    let mut line = String::new();
    BufReader::new(stream).take(8193).read_line(&mut line)?;
    ensure!(
        line.len() <= 8192 && line.ends_with('\n'),
        "Invalid encoder response"
    );
    let response: Value = serde_json::from_str(&line)?;
    ensure!(response["id"] == 1, "Unexpected encoder response");
    Ok(response)
}

fn finish_window(directory: PathBuf, output: PathBuf) -> Result<()> {
    let reply = encoder_request(
        &directory.join("encoder.sock"),
        "stop-replay-recording",
        Duration::from_secs(20),
    )?;
    ensure!(
        reply["result"] == "ok",
        "Could not save window recording: {}",
        reply["data"]
    );
    let saved = fs::canonicalize(s(&reply["data"]))?;
    ensure!(
        saved.starts_with(directory.canonicalize()?),
        "Unexpected recording path"
    );
    fs::rename(saved, output)?;
    Ok(())
}

fn wait_for_picker(child: &mut std::process::Child, ipc: &Path) -> Result<bool> {
    let deadline = Instant::now() + Duration::from_secs(150);
    let mut connection = None;
    let mut bytes = Vec::new();
    loop {
        ensure!(!STOP.load(Ordering::Relaxed), "Recording cancelled");
        if let Some(status) = child.try_wait()? {
            ensure!(
                status.success(),
                "Recorder startup failed ({status}); {}",
                fs::read_to_string(xdg("state").join("capture.log"))
                    .unwrap_or_default()
                    .lines()
                    .find(|line| line.contains("error"))
                    .unwrap_or("Inspect the Runtjik capture log")
            );
            return Ok(false);
        }
        ensure!(Instant::now() < deadline, "Window picker timed out");
        if connection.is_none() {
            if let Ok(mut stream) = UnixStream::connect(ipc) {
                stream.set_read_timeout(Some(Duration::from_millis(100)))?;
                stream.set_write_timeout(Some(Duration::from_millis(100)))?;
                writeln!(stream, "{}", json!({"id":1,"name":"stop-replay-recording"}))?;
                connection = Some(stream);
            }
        }
        if let Some(stream) = &mut connection {
            let mut buffer = [0; 1024];
            match stream.read(&mut buffer) {
                Ok(0) => {
                    // Picker cancellation can close IPC just before waitpid
                    // reports the child's exit. Classify its exit next loop.
                    connection = None;
                    bytes.clear();
                }
                Ok(n) => {
                    bytes.extend_from_slice(&buffer[..n]);
                    ensure!(bytes.len() <= 8192, "Invalid encoder response");
                    if bytes.contains(&b'\n') {
                        let reply: Value = serde_json::from_slice(&bytes)?;
                        ensure!(reply["id"] == 1, "Unexpected encoder response");
                        // The event loop replies only after the picker completes.
                        return Ok(true);
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    ()
                }
                Err(e) if recorder_disconnected(&e) => {
                    connection = None;
                    bytes.clear();
                }
                Err(e) => return Err(e.into()),
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
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
    let result = (|| -> Result<()> {
        let logdir = xdg("state");
        private_dir(&logdir)?;
        let log = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(logdir.join("capture.log"))?;
        let mut a = argv(&spec)?;
        // Keep the selected portal session alive, but start the saved video only
        // after countdown. Replay stays in RAM and is never saved.
        let portal = if spec["mode"] == "window" {
            let temp = tempfile::Builder::new()
                .prefix(".runtjik-window-")
                .tempdir_in(Path::new(s(&spec["file"])).parent().unwrap())?;
            a.pop(); // replace -o's regular file with the private replay directory
            a.push(temp.path().to_string_lossy().into_owned());
            a.extend([
                "-r".into(),
                "3".into(), // GPU Screen Recorder requires a buffer greater than 2 seconds.
                "-c".into(),
                "mp4".into(),
                "-replay-storage".into(),
                "ram".into(),
                "-ro".into(),
                temp.path().to_string_lossy().into_owned(),
                "-ipc".into(),
                temp.path()
                    .join("encoder.sock")
                    .to_string_lossy()
                    .into_owned(),
            ]);
            Some(temp)
        } else {
            None
        };
        let mut child = Command::new(&a[0])
            .args(&a[1..])
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?;
        let mut stopping = None;
        let mut finalizer = None;
        let mut workspace_check = Instant::now();
        let outcome = (|| -> Result<()> {
            if let Some(portal) = &portal {
                let ipc = portal.path().join("encoder.sock");
                if !wait_for_picker(&mut child, &ipc)? {
                    println!("{}", json!({"cancelled":true}));
                    return Ok(());
                }
                if !capture::countdown(&spec)? {
                    println!("{}", json!({"cancelled":true}));
                    return Ok(());
                }
                let reply =
                    encoder_request(&ipc, "start-replay-recording", Duration::from_secs(2))?;
                ensure!(
                    reply["result"] == "ok",
                    "Could not start window recording: {}",
                    reply["data"]
                );
            }
            std::thread::sleep(Duration::from_millis(300));
            ensure!(
                child.try_wait()?.is_none(),
                "Recorder exited during startup; inspect {}",
                logdir.join("capture.log").display()
            );
            let _shortcut = StopShortcut::install()?;
            let server = UnixListener::bind(&path)?;
            server.set_nonblocking(true)?;
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
                    let same = crate::windows::outputs()
                        .ok()
                        .and_then(|ms| ms.into_iter().find(|m| m["name"] == spec["output"]))
                        .is_some_and(|m| capture::workspace(&m)["id"] == spec["workspace"]);
                    if !same {
                        STOP.store(true, Ordering::Relaxed);
                    }
                }
                if STOP.load(Ordering::Relaxed) && stopping.is_none() {
                    if let Some(portal) = &portal {
                        let directory = portal.path().to_path_buf();
                        let output = PathBuf::from(s(&spec["file"]));
                        // Continue answering bar/launcher status while the
                        // encoder flushes and closes the MP4.
                        finalizer =
                            Some(std::thread::spawn(move || finish_window(directory, output)));
                    } else {
                        unsafe {
                            libc::kill(child.id() as i32, libc::SIGINT);
                        }
                    }
                    stopping = Some(Instant::now());
                }
                if finalizer.as_ref().is_some_and(|h| h.is_finished()) {
                    finalizer
                        .take()
                        .unwrap()
                        .join()
                        .map_err(|_| anyhow::anyhow!("Recording finalizer failed"))??;
                    unsafe {
                        libc::kill(child.id() as i32, libc::SIGINT);
                    }
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
            notify_finished(s(&spec["file"]), success, &logdir);
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
        if let Some(finalizer) = finalizer {
            let _ = finalizer.join();
        }
        if outcome.is_err() {
            // Retain any partial video for recovery if the encoder could not
            // finish; cancellation before recording still removes its scratch dir.
            if let Some(portal) = portal {
                let _ = portal.keep();
            }
        }
        outcome
    })();
    let _ = fs::remove_file(path);
    result
}
