//! Firefox native messaging framing and per-connection Unix bridge.
use crate::util::*;
use anyhow::{bail, ensure, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::{fs::MetadataExt, net::UnixListener},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use uuid::Uuid;
const MAX: usize = 4 * 1024 * 1024;
fn read_message(stream: &mut impl Read) -> Result<Value> {
    let mut header = [0; 4];
    stream.read_exact(&mut header)?;
    let size = u32::from_ne_bytes(header) as usize;
    ensure!(size <= MAX, "Native message too large");
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}
fn write_message(stream: &mut impl Write, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= MAX, "Native message too large");
    stream.write_all(&(bytes.len() as u32).to_ne_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}
fn marked_window(marker: &str) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let rows: Vec<_> = crate::windows::clients()?
            .into_iter()
            .filter(|c| s(&c["title"]).starts_with(marker))
            .collect();
        if rows.len() == 1 {
            return Ok(rows[0].clone());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    bail!("Could not identify the browser window in Hyprland")
}
pub fn serve() -> Result<()> {
    unsafe {
        libc::umask(0o077);
    }
    let directory = crate::browser::directory()?;
    private_dir(&directory)?;
    let meta = fs::symlink_metadata(&directory)?;
    ensure!(
        !meta.file_type().is_symlink()
            && meta.uid() == unsafe { libc::getuid() }
            && meta.mode() & 0o077 == 0,
        "Browser runtime directory must be private"
    );
    let path = directory.join(format!("{}.sock", Uuid::new_v4().simple()));
    let server = UnixListener::bind(&path)?;
    server.set_nonblocking(true)?;
    let stopped = Arc::new(AtomicBool::new(false));
    let done = stopped.clone();
    let (sender, inbox) = mpsc::sync_channel(16);
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        while let Ok(value) = read_message(&mut input) {
            if sender.try_send(value).is_err() {
                break;
            }
        }
        done.store(true, Ordering::Relaxed);
    });
    let result = (|| -> Result<()> {
        let exchange = |mut message: Value| -> Result<Value> {
            let id = Uuid::new_v4().simple().to_string();
            message["id"] = json!(id);
            write_message(&mut io::stdout().lock(), &message)?;
            let deadline = Instant::now() + Duration::from_secs(4);
            while Instant::now() < deadline {
                match inbox.recv_timeout(Duration::from_millis(100)) {
                    Ok(value) if value["id"] == id => {
                        if let Some(error) = value.get("error") {
                            bail!("{}", s(error));
                        }
                        return Ok(value);
                    }
                    Ok(_) => (),
                    Err(mpsc::RecvTimeoutError::Disconnected) => bail!("Browser disconnected"),
                    Err(_) => {
                        if stopped.load(Ordering::Relaxed) {
                            bail!("Browser disconnected");
                        }
                    }
                }
            }
            bail!("Browser response timed out")
        };
        while !stopped.load(Ordering::Relaxed) {
            let (mut client, _) = match server.accept() {
                Ok(c) => c,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(25));
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            client.set_read_timeout(Some(Duration::from_secs(12)))?;
            client.set_write_timeout(Some(Duration::from_secs(12)))?;
            let action = || -> Result<Value> {
                let mut bytes = Vec::new();
                BufReader::new(&client)
                    .take(65537)
                    .read_until(b'\n', &mut bytes)?;
                ensure!(bytes.len() <= 65536, "Request too large");
                let mut message: Value = serde_json::from_slice(&bytes)?;
                let op = s(&message["op"]);
                ensure!(
                    ["list", "focus", "details", "close"].contains(&op),
                    "Unknown browser operation"
                );
                let close = op == "close" && message["tab"].is_null();
                if op != "focus" && !close {
                    return exchange(message);
                }
                let marker = format!("[runtjik-{}] ", Uuid::new_v4().simple());
                let window = message["window"].clone();
                message["op"] = json!("focus");
                message["marker"] = json!(marker);
                let result = (|| -> Result<Value> {
                    let response = exchange(message)?;
                    let client = marked_window(&marker)?;
                    if close {
                        let target =
                            serde_json::to_string(&format!("address:{}", s(&client["address"])))?;
                        command(&[
                            "hyprctl",
                            "dispatch",
                            &format!("hl.dsp.window.close({{window={target}}})"),
                        ])?;
                        Ok(
                            json!({"message":"Hyprland close requested; Zen may ask for confirmation"}),
                        )
                    } else {
                        crate::windows::focus(&client)?;
                        Ok(response)
                    }
                })();
                let _ = exchange(json!({"op":"release","window":window}));
                result
            };
            let response = action().unwrap_or_else(|e| json!({"error":e.to_string()}));
            let _ = writeln!(client, "{response}");
        }
        Ok(())
    })();
    let _ = fs::remove_file(path);
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_frames_roundtrip_and_reject_oversize() {
        let value = json!({"id":"synthetic","title":"中文"});
        let mut bytes = Vec::new();
        write_message(&mut bytes, &value).unwrap();
        assert_eq!(read_message(&mut bytes.as_slice()).unwrap(), value);
        assert!(read_message(&mut ((MAX + 1) as u32).to_ne_bytes().as_slice()).is_err());
        assert!(read_message(&mut [1u8, 0, 0, 0].as_slice()).is_err());
    }
}

pub fn register(args: &[String]) -> Result<()> {
    use std::{os::unix::fs::PermissionsExt, path::PathBuf};
    let mut root = home().join(".local/lib/runtjik");
    let mut directory = home().join(".mozilla/native-messaging-hosts");
    let mut i = 0;
    while i < args.len() {
        let value = args
            .get(i + 1)
            .ok_or_else(|| anyhow::anyhow!("Missing registration path"))?;
        match args[i].as_str() {
            "--runtjik-dir" => root = PathBuf::from(value),
            "--manifest-dir" => directory = PathBuf::from(value),
            _ => bail!("Unknown registration option"),
        }
        i += 2;
    }
    let root = fs::canonicalize(root)?;
    let host = root.join("runtjik-core");
    ensure!(host.is_file(), "Install Runtjik first");
    let launcher = root.join("browser-native-host");
    let quoted = format!("'{}'", host.to_string_lossy().replace('\'', "'\\''"));
    atomic(
        &launcher,
        format!("#!/bin/sh\nexec {quoted} browser-host\n").as_bytes(),
    )?;
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700))?;
    private_dir(&directory)?;
    let manifest = directory.join("org.tjikos.runtjik_browser.json");
    atomic(
        &manifest,
        &serde_json::to_vec_pretty(
            &json!({"name":"org.tjikos.runtjik_browser","description":"Runtjik browser bridge","path":launcher,"type":"stdio","allowed_extensions":["runtjik-browser@tjikos.local"]}),
        )?,
    )?;
    println!("Registered {}", manifest.display());
    Ok(())
}
