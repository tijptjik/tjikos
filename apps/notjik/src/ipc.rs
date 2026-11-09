//! Private, bounded native request transport. Never retry a submitted mutation.
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
pub const LIMIT: usize = 131072;
pub fn path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("NOTJIK_SOCKET") {
        return Ok(PathBuf::from(path));
    }
    Ok(
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is required")?)
            .join("notjik.sock"),
    )
}
fn read(stream: &mut UnixStream, limit: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = vec![];
    let n = BufReader::new(stream)
        .take(limit as u64 + 1)
        .read_until(b'\n', &mut bytes)?;
    ensure!(
        n > 0 && n <= limit + 1 && bytes.last() == Some(&b'\n'),
        "Invalid or oversized IPC message"
    );
    Ok(bytes)
}
pub fn request(value: &Value) -> Result<Option<Value>> {
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() && std::env::var_os("NOTJIK_SOCKET").is_none()
    {
        return Ok(None);
    }
    request_at(&path()?, value)
}
pub fn request_at(path: &std::path::Path, value: &Value) -> Result<Option<Value>> {
    let mut stream = match UnixStream::connect(path) {
        Ok(s) => s,
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(None)
        }
        Err(e) => return Err(e.into()),
    };
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= LIMIT, "Request exceeds 128 KiB");
    stream.write_all(&bytes)?;
    stream.write_all(b"\n")?;
    let response: Value = serde_json::from_slice(&read(&mut stream, 16 * 1024 * 1024)?)?;
    ensure!(
        response["ok"] == true,
        "{}",
        response["error"]
            .as_str()
            .unwrap_or("Native request failed")
    );
    Ok(Some(response))
}
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
                            let value: Value = serde_json::from_slice(&read(&mut stream, LIMIT)?)?;
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
