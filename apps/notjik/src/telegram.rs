use crate::util::*;
use anyhow::{bail, ensure, Result};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};
pub fn identifier(v: &Value) -> Result<i64> {
    let n = if let Some(s) = v.as_str() {
        ensure!(
            regex::Regex::new(r"^-?[0-9]+$")?.is_match(s),
            "Invalid Telegram identifier"
        );
        s.parse::<i64>()?
    } else {
        v.as_i64()
            .ok_or_else(|| anyhow::anyhow!("Invalid Telegram identifier"))?
    };
    ensure!(
        n != 0 && n.unsigned_abs() <= 9007199254740991,
        "Invalid Telegram identifier"
    );
    Ok(n)
}
pub fn socket_path() -> Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| anyhow::anyhow!("XDG_RUNTIME_DIR is required for Telegram controls"))?;
    Ok(std::env::var_os("NOTJIK_TELEGRAM_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(runtime).join("notjik-telegram/control.sock")))
}
pub fn socket_request(request: &Value) -> Result<Value> {
    ensure!(request.is_object(), "Telegram request must be an object");
    let mut raw = serde_json::to_vec(request)?;
    raw.push(b'\n');
    ensure!(raw.len() <= 32768, "Telegram request too large");
    let mut sock = UnixStream::connect(socket_path()?)?;
    sock.set_read_timeout(Some(Duration::from_secs(35)))?;
    sock.set_write_timeout(Some(Duration::from_secs(35)))?;
    let result = (|| -> Result<Value> {
        sock.write_all(&raw)?;
        let mut b = Vec::new();
        BufReader::new(&sock)
            .take(32769)
            .read_until(b'\n', &mut b)?;
        ensure!(b.len() <= 32768, "Telegram response too large");
        let v: Value = serde_json::from_slice(&b)?;
        ensure!(v.is_object(), "Invalid Telegram response");
        Ok(v)
    })();
    if result.is_err() && request["action"] == "voiceReply" {
        return Ok(
            serde_json::json!({"ok":false,"uncertain":true,"error":"No confirmation from Telegram; check the chat before retrying"}),
        );
    }
    result
}
pub fn main(root: &Path, args: &[String]) -> Result<()> {
    ensure!(
        !root.join(".synthetic-only").exists(),
        "Real Telegram access is disabled in the demo cycle"
    );
    let op = args.first().map(String::as_str).unwrap_or("help");
    let result = (|| -> Result<()> {
        match op {
            "run" | "login" => crate::bridge::run_bridge(root, op == "login"),
            "doctor" => {
                let td = crate::native::Td::new(root)?;
                let v = td.execute(&serde_json::json!({"@type":"getOption","name":"version"}))?;
                println!("TDLib: {}", v["value"].as_str().unwrap_or("available"));
                let lottie = crate::native::Library::open(root, "rlottie").is_ok();
                let available = |name: &str| {
                    std::env::var_os("PATH")
                        .into_iter()
                        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
                        .any(|p| p.join(name).is_file())
                };
                println!(
                    "Voice recording: {}",
                    if available("ffmpeg") && available("ffprobe") {
                        "ready"
                    } else {
                        "install FFmpeg and ffprobe"
                    }
                );
                println!(
                    "Animated stickers: {}",
                    if lottie && available("ffmpeg") {
                        "native rlottie available"
                    } else {
                        "run telegram setup"
                    }
                );
                println!(
                    "Account configuration: {}",
                    if telegram_dir().join("credentials.json").is_file() {
                        "present"
                    } else {
                        "not signed in"
                    }
                );
                Ok(())
            }
            "setup" => {
                let script = if root.join("install-telegram.sh").exists() {
                    root.join("install-telegram.sh")
                } else {
                    root.join("scripts/install-telegram.sh")
                };
                ensure!(
                    std::process::Command::new("bash")
                        .arg(script)
                        .status()?
                        .success(),
                    "Telegram setup failed"
                );
                Ok(())
            }
            "status" | "action" => {
                let request = if op == "status" {
                    serde_json::json!({"action":"status"})
                } else {
                    let mut raw = Vec::new();
                    std::io::stdin()
                        .lock()
                        .take(32769)
                        .read_until(b'\n', &mut raw)?;
                    ensure!(raw.len() <= 32768, "Telegram request too large");
                    serde_json::from_slice(&raw)?
                };
                let v = socket_request(&request)?;
                emit(&v);
                if v["ok"] != true {
                    std::process::exit(1)
                }
                Ok(())
            }
            "help" | "--help" | "-h" => {
                println!("notjikctl telegram <setup|login|run|status|action|doctor>");
                Ok(())
            }
            _ => bail!("Unknown Telegram operation"),
        }
    })();
    if op == "action" {
        if let Err(e) = &result {
            emit(&serde_json::json!({"ok":false,"error":e.to_string()}));
        }
    }
    result
}
