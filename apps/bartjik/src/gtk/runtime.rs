//! Own the core and read-only activity helpers, with bounded restartable pipes.
use super::Event;
use std::os::fd::AsRawFd;
fn write_command(input: &mut std::process::ChildStdin, value: &Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    if bytes.len() > 1_048_576 {
        return Err("Backend command too large".into());
    }
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    let mut offset = 0;
    while offset < bytes.len() {
        match input.write(&bytes[offset..]) {
            Ok(0) => return Err("Backend pipe closed".into()),
            Ok(n) => offset += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    // Partial JSON cannot be replayed safely; force the owned core to reconnect.
                    return Err("Backend write timed out".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
#[derive(Clone)]
pub struct Runtime {
    inner: Arc<Inner>,
}
struct Inner {
    stop: AtomicBool,
    queue: std::sync::mpsc::SyncSender<Value>,
    input: Mutex<Option<std::process::ChildStdin>>,
    screens: Mutex<Value>,
    pids: Mutex<Vec<u32>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for pid in self.pids.get_mut().unwrap().drain(..) {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGTERM);
            }
        }
    }
}
impl Runtime {
    pub fn start(tx: async_channel::Sender<Event>, demo: bool, activity: bool) -> Self {
        let (queue, commands) = std::sync::mpsc::sync_channel(32);
        let inner = Arc::new(Inner {
            queue,
            stop: AtomicBool::new(false),
            input: Mutex::new(None),
            screens: Mutex::new(json!([])),
            pids: Mutex::new(vec![]),
        });
        let weak = Arc::downgrade(&inner);
        let writer = weak.clone();
        let errors = tx.clone();
        std::thread::spawn(move || {
            while let Ok(value) = commands.recv() {
                let Some(inner) = writer.upgrade() else {
                    break;
                };
                let result = inner
                    .input
                    .lock()
                    .unwrap()
                    .as_mut()
                    .ok_or("Backend unavailable".to_string())
                    .and_then(|input| write_command(input, &value));
                if let Err(error) = result {
                    inner.input.lock().unwrap().take();
                    let _ = errors.send_blocking(Event::Result(error));
                }
            }
        });
        let binary = crate::util::root().join("bin/bartjik-gtk-core");
        let fixture = demo
            .then(|| std::env::var_os("BARTJIK_GTK_ACTIVITY_FIXTURE"))
            .flatten();
        for kind in std::iter::once("backend").chain(if fixture.is_some() {
            vec!["hyprvoice-status"]
        } else if activity && !demo {
            vec!["hyprvoice-status", "screen-record-status"]
        } else {
            vec![]
        }) {
            let weak = weak.clone();
            let fixture = fixture.clone();
            let binary = binary.clone();
            let tx = tx.clone();
            std::thread::spawn(move || loop {
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                if inner.stop.load(Ordering::Relaxed) {
                    break;
                }
                let mut command = Command::new(&binary);
                command
                    .env("BARTJIK_FRONTEND", "gtk")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null());
                if kind == "backend" {
                    command.arg("serve");
                    if demo {
                        command.arg("--demo");
                    }
                } else {
                    command.args(["helper", kind]);
                    if let Some(path) = &fixture {
                        command.arg(path);
                    }
                }
                use std::os::unix::process::CommandExt;
                command.process_group(0);
                let parent = std::process::id();
                // These children are the owned core/status readers, never capture workers.
                // Do not leave duplicate providers behind if GTK crashes.
                unsafe {
                    command.pre_exec(move || {
                        if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                        if libc::getppid() as u32 != parent {
                            return Err(std::io::Error::other("GTK parent exited"));
                        }
                        Ok(())
                    });
                }
                match command.spawn() {
                    Ok(mut child) => {
                        let pid = child.id();
                        inner.pids.lock().unwrap().push(pid);
                        if kind == "backend" {
                            let mut input = child.stdin.take().unwrap();
                            let _ = writeln!(
                                input,
                                "{}",
                                json!({"op":"screens","screens":*inner.screens.lock().unwrap()})
                            );
                            unsafe {
                                let fd = input.as_raw_fd();
                                let flags = libc::fcntl(fd, libc::F_GETFL);
                                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                            }
                            *inner.input.lock().unwrap() = Some(input);
                        }
                        let stdout = child.stdout.take().unwrap();
                        drop(inner);
                        let mut reader = BufReader::new(stdout);
                        loop {
                            let mut raw = vec![];
                            match (&mut reader).take(4_194_305).read_until(b'\n', &mut raw) {
                                Ok(0) | Err(_) => break,
                                Ok(_) if raw.len() > 4_194_304 => break,
                                _ => (),
                            }
                            if let Ok(value) = serde_json::from_slice(&raw) {
                                if tx
                                    .send_blocking(if kind == "backend" {
                                        Event::Backend(value)
                                    } else {
                                        Event::Activity(kind.into(), value)
                                    })
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                        let _ = child.kill();
                        let _ = child.wait();
                        if let Some(inner) = weak.upgrade() {
                            inner.pids.lock().unwrap().retain(|p| *p != pid);
                            if kind == "backend" {
                                inner.input.lock().unwrap().take();
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send_blocking(Event::Result(format!("{kind}: {e}")));
                    }
                }
                let _ = tx.send_blocking(if kind == "backend" {
                    Event::Backend(
                        json!({"ready":false,"error":"Backend disconnected; reconnecting"}),
                    )
                } else {
                    Event::Activity(kind.into(), json!({"available":false}))
                });
                std::thread::sleep(Duration::from_secs(1));
            });
        }
        Self { inner }
    }
    pub fn send(&self, value: Value) -> Result<(), String> {
        self.inner
            .queue
            .try_send(value)
            .map_err(|_| "Backend command queue unavailable or full".into())
    }
    pub fn screens(&self, value: Value) {
        let mut screens = self.inner.screens.lock().unwrap();
        if *screens != value {
            *screens = value.clone();
            drop(screens);
            let _ = self.send(json!({"op":"screens","screens":value}));
        }
    }
}
