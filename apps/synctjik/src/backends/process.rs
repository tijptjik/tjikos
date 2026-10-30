use anyhow::{bail, ensure, Context, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{MetadataExt, OpenOptionsExt},
            process::CommandExt,
        },
    },
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

pub struct Runner {
    cancelled: Arc<AtomicBool>,
    signals: Vec<signal_hook::SigId>,
}
impl Runner {
    pub fn new() -> Result<Self> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut signals = Vec::new();
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            signals.push(signal_hook::flag::register(signal, cancelled.clone())?);
        }
        Ok(Self { cancelled, signals })
    }
    pub fn check(&self) -> Result<()> {
        ensure!(
            !self.cancelled.load(Ordering::Relaxed),
            "Operation interrupted"
        );
        Ok(())
    }
    pub fn run(&self, command: &mut Command) -> Result<Vec<u8>> {
        self.guarded(command, None, false).map(|(_, data)| data)
    }
    pub fn guarded(
        &self,
        command: &mut Command,
        guard: Option<&mut Child>,
        allow_failure: bool,
    ) -> Result<(i32, Vec<u8>)> {
        self.execute(command, guard, allow_failure, None, None)
    }
    /// Stream large database exports to a caller-owned private file.
    pub fn run_to(&self, command: &mut Command, output: &File) -> Result<()> {
        self.execute(command, None, false, Some(output), None)?;
        output.sync_all()?;
        Ok(())
    }
    /// Preserve dependency diagnostics in a private log without exposing them.
    pub fn logged(&self, command: &mut Command, log: &File) -> Result<(i32, Vec<u8>)> {
        self.execute(command, None, true, None, Some(log))
    }
    /// Retain private diagnostics while keeping the mount/coordination guard.
    pub fn guarded_logged(
        &self,
        command: &mut Command,
        guard: Option<&mut Child>,
        log: &File,
    ) -> Result<(i32, Vec<u8>)> {
        self.execute(command, guard, true, None, Some(log))
    }
    fn execute(
        &self,
        command: &mut Command,
        mut guard: Option<&mut Child>,
        allow_failure: bool,
        output: Option<&File>,
        log: Option<&File>,
    ) -> Result<(i32, Vec<u8>)> {
        self.check()?;
        if let Some(guard) = guard.as_mut() {
            ensure!(
                guard.try_wait()?.is_none(),
                "Lost backup coordination lock; refusing to start operation"
            );
        }
        // Files avoid pipe deadlocks and keep subprocess diagnostics private.
        let mut out = tempfile::tempfile()?;
        let mut err = tempfile::tempfile()?;
        command
            .stdout(output.unwrap_or(&out).try_clone()?)
            .stderr(err.try_clone()?)
            .process_group(0);
        let mut child = ManagedChild(
            command
                .spawn()
                .context("Unable to start backup dependency")?,
            true,
        );
        loop {
            self.check()?;
            if let Some(guard) = guard.as_mut() {
                ensure!(
                    guard.try_wait()?.is_none(),
                    "Lost backup coordination lock; stopping backup"
                );
            }
            if let Some(status) = child.0.try_wait()? {
                let code = status.code().unwrap_or(-1);
                ensure!(
                    status.success() || allow_failure,
                    "Backup dependency failed (exit {code}); output withheld"
                );
                out.seek(SeekFrom::Start(0))?;
                let mut data = Vec::new();
                out.read_to_end(&mut data)?;
                if let Some(mut log) = log {
                    use std::io::Write;
                    log.write_all(&data)?;
                    err.seek(SeekFrom::Start(0))?;
                    std::io::copy(&mut err, &mut log)?;
                    log.sync_all()?;
                }
                return Ok((code, data));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    pub fn interactive(&self, command: &mut Command) -> Result<()> {
        self.visible(command, false)
    }
    /// Keep cutover descendants inside the lifetime of the held legacy locks.
    pub fn supervised(&self, command: &mut Command) -> Result<()> {
        command.process_group(0);
        self.visible(command, true)
    }
    fn visible(&self, command: &mut Command, group: bool) -> Result<()> {
        self.check()?;
        // age must retain terminal access for its passphrase prompt.
        let mut child = ManagedChild(
            command
                .spawn()
                .context("Unable to start interactive dependency")?,
            group,
        );
        loop {
            self.check()?;
            if let Some(s) = child.0.try_wait()? {
                ensure!(s.success(), "Interactive dependency failed");
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
impl Drop for Runner {
    fn drop(&mut self) {
        for id in self.signals.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}
pub struct ManagedChild(pub Child, pub bool);
impl Drop for ManagedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            // Children started with process_group(0) cannot outlive their lock.
            unsafe {
                libc::kill(
                    if self.1 {
                        -(self.0.id() as i32)
                    } else {
                        self.0.id() as i32
                    },
                    libc::SIGTERM,
                );
            }
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while self.0.try_wait().ok().flatten().is_none() && std::time::Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(100));
            }
            // Kill the entire group even when its leader exits first.
            unsafe {
                libc::kill(
                    if self.1 {
                        -(self.0.id() as i32)
                    } else {
                        self.0.id() as i32
                    },
                    libc::SIGKILL,
                );
            }
            let _ = self.0.wait();
        }
    }
}
pub struct Lock(File);
impl Lock {
    pub fn take(path: &Path, wait: bool, runner: &Runner) -> Result<Self> {
        // /run/lock is shared with other services: never chmod an existing parent.
        let parent = path.parent().context("Missing lock directory")?;
        if !parent.exists() {
            crate::util::private_dir(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(Self(file));
            }
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::EWOULDBLOCK) {
                return Err(error.into());
            }
            if !wait {
                return Err(Busy("Another backup or sync is running".into()).into());
            }
            runner.check()?;
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
#[derive(Debug)]
pub struct Busy(pub String);
impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Busy {}
pub fn command(args: &[String]) -> Command {
    let mut c = Command::new(&args[0]);
    c.args(&args[1..]);
    c
}
pub fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}
pub fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
pub fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is unavailable")
}
pub fn available(name: &str) -> bool {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).any(|d| {
        fs::metadata(d.join(name))
            .map(|m| m.is_file() && m.mode() & 0o111 != 0)
            .unwrap_or(false)
    })
}
pub fn absolute(p: &Path) -> Result<()> {
    ensure!(
        p.is_absolute()
            && !p
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir)),
        "Policy paths must be absolute without traversal"
    );
    Ok(())
}
pub fn no_symlinks(p: &Path) -> Result<()> {
    absolute(p)?;
    for ancestor in p.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(m) => ensure!(
                !m.file_type().is_symlink(),
                "Refusing a symlink in a managed path"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
pub fn secret(p: &Path) -> Result<String> {
    no_symlinks(p)?;
    let mut file = crate::util::read_file(p).context("Credential unavailable")?;
    let m = file.metadata()?;
    ensure!(
        m.uid() == unsafe { libc::getuid() } && m.mode() & 0o077 == 0,
        "Credential must be an owner-only regular file"
    );
    let mut s = String::new();
    file.read_to_string(&mut s)
        .context("Invalid credential encoding")?;
    ensure!(!s.trim().is_empty(), "Empty credential");
    Ok(s.trim().into())
}
pub fn mounted(p: &Path) -> bool {
    // mountpoint uses /proc mount information, including FUSE and bind mounts.
    Command::new("mountpoint")
        .arg("-q")
        .arg(p)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
pub fn free_space(p: &Path) -> Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(p.as_os_str().as_bytes())?;
    let mut data = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(c.as_ptr(), data.as_mut_ptr()) } != 0 {
        bail!("Unable to check available backup space");
    }
    let data = unsafe { data.assume_init() };
    Ok(data.f_bavail.saturating_mul(data.f_frsize))
}
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
pub fn shell_join(args: &[String]) -> String {
    args.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ")
}
