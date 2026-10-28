use crate::config::Recipe;
use anyhow::{bail, Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::{
    fs::OpenOptionsExt,
    process::{CommandExt, ExitStatusExt},
};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub fn private_file(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?)
}

pub struct Lock(File);
impl Lock {
    pub fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .with_context(|| format!("cannot open lock {}", path.display()))?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            bail!("workflow/repository is busy: {}", path.display());
        }
        Ok(Self(file))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

struct Foreground(Option<i32>);
impl Drop for Foreground {
    fn drop(&mut self) {
        if let Some(group) = self.0 {
            unsafe {
                libc::tcsetpgrp(0, group);
            }
        }
    }
}

pub enum Input<'a> {
    Repeat(&'a str),
}

pub fn run(
    argv: &[String],
    recipe: &Recipe,
    cancelled: &AtomicBool,
    run_dir: &Path,
    log: &File,
    output: Option<&Path>,
    input: Option<Input<'_>>,
) -> Result<i32> {
    if cancelled.load(Ordering::SeqCst) {
        bail!("interrupted");
    }
    let argv: Vec<_> = argv
        .iter()
        .map(|s| s.replace("{run_dir}", &run_dir.to_string_lossy()))
        .collect();
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(&recipe.cwd)
        .envs(&recipe.env)
        .env("TJIKUP_RUN_DIR", run_dir)
        .env(
            "TJIKUP_REPORT_FILE",
            run_dir.join(format!("{}.report", recipe.id)),
        )
        .env("TJIKUP_NO_SPIN", "1")
        .env("TJIKUP_STATUS_LINES", "1")
        .env("TJIKUP_NO_SECTIONS", "1")
        .env("TJIKUP_STATUS_SECTIONS", "1")
        .process_group(0);
    command.stdin(if input.is_some() {
        Stdio::piped()
    } else if recipe.interactive {
        Stdio::inherit()
    } else {
        Stdio::null()
    });
    command.stdout(if let Some(path) = output {
        Stdio::from(private_file(path)?)
    } else if recipe.interactive {
        Stdio::piped()
    } else {
        Stdio::from(log.try_clone()?)
    });
    command.stderr(if recipe.interactive {
        Stdio::inherit()
    } else {
        Stdio::from(log.try_clone()?)
    });
    let interactive = recipe.interactive && unsafe { libc::isatty(0) } == 1;
    let _foreground = Foreground(if interactive {
        Some(unsafe { libc::tcgetpgrp(0) })
    } else {
        None
    });
    if interactive {
        // setpgid happens before pre_exec. Give interactive tools the terminal before they read it.
        unsafe {
            command.pre_exec(|| {
                if libc::tcsetpgrp(0, libc::getpgrp()) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("cannot start {}", argv[0]))?;
    if let Some(stdin) = &child.stdin {
        unsafe {
            libc::fcntl(stdin.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
        }
    }
    if let Some(stdout) = &child.stdout {
        unsafe {
            libc::fcntl(stdout.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
        }
    }
    let mut filter = StatusFilter::default();
    let mut input_offset = 0;
    let started = Instant::now();
    loop {
        drain_output(&mut child.stdout, &mut filter, log);
        if let Some(status) = child.try_wait()? {
            if status.signal() == Some(libc::SIGINT) || status.signal() == Some(libc::SIGTERM) {
                cancelled.store(true, Ordering::SeqCst);
            }
            drain_output(&mut child.stdout, &mut filter, log);
            filter.finish();
            return Ok(status.code().unwrap_or(128 + status.signal().unwrap_or(1)));
        }
        if cancelled.load(Ordering::SeqCst)
            || started.elapsed() >= Duration::from_secs(recipe.timeout)
        {
            // Kill the entire owned process group, including grandchildren, then reap it.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            child.wait()?;
            drain_output(&mut child.stdout, &mut filter, log);
            filter.finish();
            if cancelled.load(Ordering::SeqCst) {
                bail!("interrupted");
            }
            bail!("command timed out after {}s", recipe.timeout);
        }
        if let (Some(Input::Repeat(text)), Some(stdin)) = (&input, &child.stdin) {
            let bytes = text.as_bytes();
            if !bytes.is_empty() {
                let count = unsafe {
                    libc::write(
                        stdin.as_raw_fd(),
                        bytes[input_offset..].as_ptr().cast(),
                        bytes.len() - input_offset,
                    )
                };
                if count > 0 {
                    input_offset = (input_offset + count as usize) % bytes.len();
                }
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Interactive tools keep foreground stdin and stderr. Only their stdout status
/// protocol is consumed; ordinary diagnostics and unterminated prompts pass on.
#[derive(Default)]
struct StatusFilter {
    pending: Vec<u8>,
    passthrough: bool,
    since: Option<Instant>,
}
impl StatusFilter {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.passthrough {
                let _ = std::io::stderr().write_all(&[byte]);
                if byte == b'\n' {
                    self.passthrough = false;
                }
                continue;
            }
            self.since.get_or_insert_with(Instant::now);
            self.pending.push(byte);
            if byte == b'\n' {
                if crate::report::Event::line(&String::from_utf8_lossy(&self.pending)).is_none() {
                    let _ = std::io::stderr().write_all(&self.pending);
                }
                self.pending.clear();
                self.since = None;
            } else if self.pending.len() >= 65536 {
                self.flush_prompt();
            }
        }
        self.prompt();
    }
    fn prompt(&mut self) {
        let text = crate::report::plain(&String::from_utf8_lossy(&self.pending));
        if crate::report::Event::line(&text).is_some() {
            return;
        }
        // Allow split writes inside the verb/UTF-8 marker, but cap ambiguous
        // uppercase text so a real unterminated prompt cannot deadlock input.
        let mut fields = text.trim_start().splitn(2, char::is_whitespace);
        let verb = fields.next().unwrap_or_default();
        let suffix = fields.next().unwrap_or_default().trim();
        let partial_status = !verb.is_empty()
            && verb.len() <= 12
            && verb.bytes().all(|b| b.is_ascii_uppercase())
            && (suffix.is_empty() || suffix == "�");
        let delay = if partial_status { 250 } else { 40 };
        if self
            .since
            .is_some_and(|at| at.elapsed() >= Duration::from_millis(delay))
        {
            self.flush_prompt();
        }
    }
    fn flush_prompt(&mut self) {
        let _ = std::io::stderr().write_all(&self.pending);
        let _ = std::io::stderr().flush();
        self.pending.clear();
        self.since = None;
        self.passthrough = true;
    }
    fn finish(&mut self) {
        if crate::report::Event::line(&String::from_utf8_lossy(&self.pending)).is_none() {
            let _ = std::io::stderr().write_all(&self.pending);
        }
        self.pending.clear();
        let _ = std::io::stderr().flush();
    }
}
fn drain_output(
    stdout: &mut Option<std::process::ChildStdout>,
    filter: &mut StatusFilter,
    log: &File,
) {
    if let Some(stdout) = stdout {
        let mut bytes = [0; 8192];
        // Bound each poll so a noisy child cannot starve cancellation/timeouts.
        for _ in 0..32 {
            match stdout.read(&mut bytes) {
                Ok(0) => break,
                Ok(count) => {
                    let _ = (&*log).write_all(&bytes[..count]);
                    filter.write(&bytes[..count]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    }
    filter.prompt();
}
