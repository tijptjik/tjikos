//! Blocking core I/O lives off GTK's main thread. Superseded queries are skipped.
use crate::Event;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::os::unix::process::CommandExt;
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

pub struct Request {
    pub generation: u64,
    pub value: Value,
}
pub struct Worker {
    pub send: async_channel::Sender<Request>,
    pub generation: Arc<AtomicU64>,
    child: Arc<std::sync::Mutex<Option<Child>>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.send.close();
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn read(reader: &mut impl BufRead) -> Result<Value> {
    let mut line = String::new();
    anyhow::ensure!(reader.read_line(&mut line)? != 0, "Rust backend exited");
    Ok(serde_json::from_str(&line)?)
}
impl Worker {
    pub fn start(demo: bool, events: async_channel::Sender<Event>) -> Self {
        let (send, receive) = async_channel::bounded::<Request>(32);
        let generation = Arc::new(AtomicU64::new(0));
        let latest = generation.clone();
        let child = Arc::new(std::sync::Mutex::new(None));
        let process = child.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                let home = std::env::var_os("RUNTJIK_HOME")
                    .map(PathBuf::from)
                    .unwrap_or(
                        std::env::current_exe()?
                            .parent()
                            .context("Executable directory missing")?
                            .to_path_buf(),
                    );
                let mut command = Command::new(home.join("runtjik-core"));
                command
                    .arg("serve")
                    .env("RUNTJIK_FRONTEND", "gtk")
                    .env_remove("RUNTJIK_RESTORE")
                    .env_remove("RUNTJIK_RECOVERY_SOCKET");
                if demo {
                    command.arg("--demo");
                }
                unsafe {
                    command.pre_exec(|| {
                        if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
                command.envs(tjik_desktop::environment());
                let mut backend = command
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::inherit())
                    .spawn()?;
                let mut input = backend.stdin.take().context("Backend stdin")?;
                let mut output = BufReader::new(backend.stdout.take().context("Backend stdout")?);
                {
                    let mut slot = process.lock().unwrap();
                    if receive.is_closed() {
                        let _ = backend.kill();
                        let _ = backend.wait();
                        return Ok(());
                    }
                    *slot = Some(backend);
                }
                let initial = read(&mut output)?;
                if demo {
                    if let Ok(ms) = std::env::var("RUNTJIK_DEMO_SETUP_MS")
                        .unwrap_or_default()
                        .parse::<u64>()
                    {
                        std::thread::sleep(std::time::Duration::from_millis(ms.min(5000)));
                    }
                }
                events.send_blocking(Event::Response(0, initial))?;
                while let Ok(request) = receive.recv_blocking() {
                    if request.value["op"] == "query"
                        && request.generation != latest.load(Ordering::Relaxed)
                    {
                        continue;
                    }
                    writeln!(input, "{}", request.value)?;
                    input.flush()?;
                    if request.value["op"] == "remember-command" {
                        continue;
                    }
                    let mut response = read(&mut output)?;
                    if request.value["op"] == "app-action" && request.value["provider"] == "Todo" {
                        response["task_request_id"] = request.value["request_id"].clone();
                    }
                    events.send_blocking(Event::Response(request.generation, response))?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                let _ = events.send_blocking(Event::Response(
                    u64::MAX,
                    json!({"kind":"fatal", "message":format!("{error:#}")}),
                ));
            }
        });
        Self {
            send,
            generation,
            child,
        }
    }
}
