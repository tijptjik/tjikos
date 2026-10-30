use crate::util::*;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
const MAX_BYTES: u64 = 4 * 1024 * 1024;
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}
fn draft_dir() -> Result<PathBuf> {
    let p = PathBuf::from(
        std::env::var("XDG_RUNTIME_DIR")
            .context("XDG_RUNTIME_DIR is required for voice recording")?,
    )
    .join("notjik-voice");
    private_dir(&p)?;
    ensure!(
        fs::symlink_metadata(&p)?.uid() == unsafe { libc::getuid() },
        "Voice draft directory is not private"
    );
    Ok(p)
}
fn token() -> Result<String> {
    let mut b = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|n| format!("{n:02x}")).collect())
}
pub fn inspect(path: &Path) -> Result<f64> {
    let raw = run(
        Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
            ])
            .arg(path),
        Duration::from_secs(10),
    )?;
    let v: Value = serde_json::from_slice(&raw)?;
    let streams = v["streams"].as_array().context("Missing audio stream")?;
    let duration = v["format"]["duration"]
        .as_str()
        .unwrap_or("0")
        .parse::<f64>()?;
    ensure!(
        streams.len() == 1
            && streams[0]["codec_name"] == "opus"
            && streams[0]["channels"] == 1
            && v["format"]["format_name"] == "ogg"
            && duration.is_finite()
            && (0.25..=301.).contains(&duration)
            && fs::metadata(path)?.len() <= MAX_BYTES,
        "Record a voice reply between 1 second and 5 minutes"
    );
    Ok(duration)
}
pub fn copy_for_upload(token: &Value, directory: &Path) -> Result<(PathBuf, f64)> {
    let token = token.as_str().context("Invalid voice draft")?;
    ensure!(
        regex::Regex::new(r"^[0-9a-f]{32}$")?.is_match(token),
        "Invalid voice draft"
    );
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(draft_dir()?.join(format!("{token}.ogg")))?;
    let m = file.metadata()?;
    ensure!(
        m.is_file()
            && m.uid() == unsafe { libc::getuid() }
            && m.nlink() == 1
            && m.mode() & 0o077 == 0
            && m.len() > 0
            && m.len() <= MAX_BYTES,
        "Invalid voice draft file"
    );
    private_dir(directory)?;
    ensure!(
        fs::read_dir(directory)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|e| e == "ogg"))
            .count()
            < 20,
        "Too many pending voice uploads; finish them in Telegram first"
    );
    let mut target = tempfile::Builder::new()
        .prefix("voice-")
        .suffix(".ogg")
        .tempfile_in(directory)?;
    let bytes = std::io::copy(&mut file.take(MAX_BYTES + 1), &mut target)?;
    ensure!(bytes <= MAX_BYTES, "Voice recording is too large");
    target.flush()?;
    let duration = inspect(target.path())?;
    let (_, path) = target.keep()?;
    Ok((path, duration))
}
pub fn record(input_args: Option<&[String]>) -> Result<()> {
    STOP.store(false, Ordering::Relaxed);
    unsafe {
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
    }
    let token = token()?;
    let path = draft_dir()?.join(format!("{token}.ogg"));
    struct Draft(PathBuf);
    impl Drop for Draft {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let _draft = Draft(path.clone());
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
    if let Some(args) = input_args {
        cmd.args(args);
    } else {
        cmd.args(["-f", "pulse", "-i", "default"]);
    }
    let mut child = cmd
        .args([
            "-t",
            "300",
            "-vn",
            "-ac",
            "1",
            "-ar",
            "48000",
            "-c:a",
            "libopus",
            "-b:a",
            "32k",
            "-application",
            "voip",
            "-progress",
            "pipe:1",
            "-nostats",
        ])
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let (tx, rx) = mpsc::sync_channel(100);
    let progress_tx = tx.clone();
    let progress = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(progress).lines() {
            match line {
                Ok(s) => {
                    if progress_tx.send((false, Some(s))).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        loop {
            let mut b = Vec::new();
            match input.by_ref().take(32769).read_until(b'\n', &mut b) {
                Ok(0) | Err(_) => {
                    let _ = tx.send((true, None));
                    break;
                }
                Ok(_) => {
                    if b.len() > 32768 {
                        let _ = tx.send((true, None));
                        break;
                    }
                    if tx
                        .send((true, Some(String::from_utf8_lossy(&b).into_owned())))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
    emit(&json!({"state":"recording","duration":0}));
    let result = (|| -> Result<()> {
        let mut ready = false;
        let mut stopped = None;
        while !STOP.load(Ordering::Relaxed) {
            if let Ok((ui, line)) = rx.recv_timeout(Duration::from_millis(100)) {
                if ui {
                    let Some(line) = line else { return Ok(()) };
                    let v: Value = serde_json::from_str(&line)?;
                    if v["action"] == "discard" {
                        return Ok(());
                    }
                    if v["action"] == "stop" && !ready && stopped.is_none() {
                        stopped = Some(Instant::now());
                        if child.try_wait()?.is_none() {
                            child.stdin.as_mut().unwrap().write_all(b"q\n")?;
                            child.stdin.as_mut().unwrap().flush()?;
                        }
                        emit(&json!({"state":"stopping"}));
                    }
                } else if !ready {
                    if let Some(n) = line
                        .as_deref()
                        .and_then(|s| s.strip_prefix("out_time_us="))
                        .and_then(|s| s.parse::<i64>().ok())
                    {
                        emit(&json!({"duration":n.max(0)/1000}));
                    }
                }
            }
            if !ready {
                ensure!(
                    !stopped.is_some_and(|t| t.elapsed() > Duration::from_secs(10)),
                    "Microphone did not stop; recording cancelled"
                );
                if let Some(status) = child.try_wait()? {
                    ensure!(
                        status.success(),
                        "Microphone unavailable; check the default input and mute settings"
                    );
                    let duration = inspect(&path)?;
                    ready = true;
                    emit(
                        &json!({"state":"ready","token":token,"source":uri(&path)?,"duration":(duration*1000.).round()}),
                    );
                }
            }
        }
        Ok(())
    })();
    if child.try_wait()?.is_none() {
        unsafe {
            libc::kill(child.id() as i32, libc::SIGTERM);
        }
        let start = Instant::now();
        while child.try_wait()?.is_none() && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(20));
        }
        if child.try_wait()?.is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
    if let Err(e) = &result {
        emit(&json!({"state":"error","error":e.to_string()}));
    }
    result
}
