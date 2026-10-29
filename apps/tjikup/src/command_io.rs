//! Bounded subprocess capture for native helpers; diagnostics never include argv.
use anyhow::{bail, Result};
use std::{
    io::{Read, Seek, SeekFrom},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}
pub fn capture(command: &mut Command, timeout: Duration) -> Result<Output> {
    let mut out = tempfile::tempfile()?;
    let mut err = tempfile::tempfile()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(err.try_clone()?)
        .process_group(0)
        .spawn()?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= timeout {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            bail!("helper command timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if out.metadata()?.len() > 32 * 1024 * 1024 || err.metadata()?.len() > 32 * 1024 * 1024 {
        bail!("helper command output too large");
    }
    out.seek(SeekFrom::Start(0))?;
    err.seek(SeekFrom::Start(0))?;
    let mut stdout = String::new();
    let mut stderr = String::new();
    out.read_to_string(&mut stdout)?;
    err.read_to_string(&mut stderr)?;
    Ok(Output {
        code: status.code().unwrap_or(1),
        stdout,
        stderr,
    })
}
pub fn checked(command: &mut Command, seconds: u64) -> Result<String> {
    let result = capture(command, Duration::from_secs(seconds))?;
    if result.code != 0 {
        bail!("helper command failed (exit {})", result.code);
    }
    Ok(result.stdout)
}
