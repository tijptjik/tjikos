//! Private, bounded native request transport. Never retry a submitted mutation.
use anyhow::{ensure, Result};
use notjik_sync::native::read_frame;
pub use notjik_sync::native::{path, request, request_at, LIMIT};
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            net::UnixListener,
        },
    },
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
pub struct Request {
    pub value: Value,
    pub reply: mpsc::SyncSender<Value>,
}
pub struct Server {
    stop: Arc<AtomicBool>,
    path: PathBuf,
    _lock: File,
}
impl Server {
    pub fn bind() -> Result<(Self, mpsc::Receiver<Request>)> {
        let path = path()?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path.with_extension("lock"))?;
        ensure!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "Native Notjik is already running"
        );
        if let Ok(m) = std::fs::symlink_metadata(&path) {
            ensure!(
                m.file_type().is_socket() && m.uid() == unsafe { libc::getuid() },
                "Unsafe IPC path"
            );
            std::fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let (tx, rx) = mpsc::sync_channel::<Request>(100);
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let result = (|| -> Result<Value> {
                            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                            stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                            let value: Value =
                                serde_json::from_slice(&read_frame(&mut stream, LIMIT)?)?;
                            ensure!(value.is_object(), "Request must be an object");
                            let (reply, result) = mpsc::sync_channel(1);
                            tx.try_send(Request { value, reply })?;
                            Ok(result.recv_timeout(Duration::from_secs(8))?)
                        })();
                        let response = result.unwrap_or_else(
                            |e| serde_json::json!({"ok":false,"error":e.to_string()}),
                        );
                        let _ = writeln!(stream, "{response}");
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok((
            Self {
                stop,
                path,
                _lock: lock,
            },
            rx,
        ))
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.path);
    }
}
