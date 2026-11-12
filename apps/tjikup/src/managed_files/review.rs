//! Explicit conflict decisions under the deployment lock, before any file writes.
use super::{
    conflict, disk,
    model::{Desired, Snapshot},
    State,
};
use anyhow::{bail, ensure, Context, Result};
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    os::fd::AsRawFd,
    path::Path,
    process::Command,
};

// The protection supervisor isolates children in a process group. Only its
// foreground workflow may delegate terminal ownership to this nested review.
struct Foreground {
    tty: File,
    previous: libc::pid_t,
    signal: libc::sighandler_t,
}
impl Foreground {
    fn acquire(tty: &File) -> Result<Self> {
        let previous = unsafe { libc::tcgetpgrp(tty.as_raw_fd()) };
        ensure!(
            previous >= 0,
            "conflict review requires a controlling terminal"
        );
        let current = unsafe { libc::getpgrp() };
        if previous != current {
            let delegated = std::env::var("TJIKUP_REVIEW_FOREGROUND_GROUP")
                .ok()
                .and_then(|s| s.parse::<libc::pid_t>().ok());
            ensure!(
                delegated == Some(previous),
                "conflict review requires the foreground terminal; rerun apply interactively"
            );
        }
        let owned_tty = tty.try_clone()?;
        let signal = unsafe { libc::signal(libc::SIGTTOU, libc::SIG_IGN) };
        if unsafe { libc::tcsetpgrp(tty.as_raw_fd(), current) } != 0 {
            unsafe {
                libc::signal(libc::SIGTTOU, signal);
            }
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self {
            tty: owned_tty,
            previous,
            signal,
        })
    }
}
impl Drop for Foreground {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetpgrp(self.tty.as_raw_fd(), self.previous);
            libc::signal(libc::SIGTTOU, self.signal);
        }
    }
}

// Never send file contents to recipe logs: prompts and explicit previews use
// the controlling terminal directly, including for sensitive manifests.
fn preview(tty: &mut File, local: &[u8], managed: &[u8]) -> Result<()> {
    if std::str::from_utf8(local).is_err() || std::str::from_utf8(managed).is_err() {
        writeln!(tty, "Binary contents differ; text preview unavailable.")?;
        return Ok(());
    }
    let directory = tempfile::tempdir()?;
    let left = directory.path().join("local");
    let right = directory.path().join("managed");
    std::fs::write(&left, local)?;
    std::fs::write(&right, managed)?;
    let output = Command::new("diff")
        .args(["-u", "--label", "local", "--label", "managed"])
        .arg(left)
        .arg(right)
        .output()
        .context("run diff for conflict preview")?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        bail!("conflict preview failed");
    }
    // Escaping terminal control characters prevents content from issuing terminal commands.
    for ch in String::from_utf8_lossy(&output.stdout).chars() {
        if ch.is_control() && ch != '\n' && ch != '\t' {
            write!(tty, "{}", ch.escape_default())?;
        } else {
            write!(tty, "{ch}")?;
        }
    }
    if output.stdout.is_empty() {
        writeln!(tty, "Content is identical; file metadata differs.")?;
    }
    Ok(())
}

pub(super) fn resolve(root: &Path, entries: &[Desired], state: &mut State) -> Result<()> {
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .context("open terminal for conflict review")?;
    let _foreground = Foreground::acquire(&tty)?;
    let mut input = BufReader::new(tty.try_clone()?);
    for entry in entries {
        let local = disk::read(root, &entry.path)?;
        if state.kept_local.get(&entry.path) == Some(&entry.snapshot)
            || !conflict(entry, &local.snapshot, state)
        {
            continue;
        }
        writeln!(tty, "\nLocal changes: {:?}", entry.path)?;
        writeln!(
            tty,
            "Keep local preserves this file until its managed version changes."
        )?;
        writeln!(
            tty,
            "Use managed backs up the current file in a private recovery receipt."
        )?;
        if entry.sensitive {
            writeln!(
                tty,
                "Sensitive file: preview may show secrets, only on this terminal."
            )?;
        }
        loop {
            write!(
                tty,
                "[d] Review difference  [k] Keep local  [m] Use managed  [q] Cancel (default): "
            )?;
            tty.flush()?;
            let mut choice = String::new();
            if input.read_line(&mut choice)? == 0 {
                bail!("conflict review cancelled; no files applied");
            }
            match choice.trim().to_ascii_lowercase().as_str() {
                "d" => {
                    writeln!(
                        tty,
                        "Local: {:?}\nManaged: {:?}",
                        local.snapshot, entry.snapshot
                    )?;
                    if matches!(local.snapshot, Snapshot::File { .. } | Snapshot::Absent)
                        && matches!(entry.snapshot, Snapshot::File { .. } | Snapshot::Absent)
                    {
                        preview(&mut tty, &local.bytes, &entry.bytes)?;
                    }
                }
                "k" => {
                    state
                        .kept_local
                        .insert(entry.path.clone(), entry.snapshot.clone());
                    break;
                }
                "m" => {
                    // Only this observed version is approved. apply rechecks it before
                    // backup and atomic replacement; edits made during review still fail.
                    state.kept_local.remove(&entry.path);
                    state
                        .files
                        .insert(entry.path.clone(), local.snapshot.clone());
                    break;
                }
                "" | "q" => bail!("conflict review cancelled; no files applied"),
                _ => writeln!(tty, "Choose d, k, m, or q.")?,
            }
        }
    }
    Ok(())
}
