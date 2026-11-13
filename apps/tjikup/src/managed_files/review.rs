//! Explicit conflict decisions under the deployment lock, before any file writes.
use super::{
    conflict, disk,
    model::{Desired, Snapshot},
    State,
};
use anyhow::{bail, ensure, Context, Result};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::fd::AsRawFd,
    path::Path,
    process::{Command, Stdio},
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

// Keep single-key mode scoped to the prompt, restoring it before rendering or
// returning an error. Handle Ctrl-C ourselves so cancellation also runs Drop.
struct KeyMode {
    tty: File,
    previous: libc::termios,
}

impl KeyMode {
    fn acquire(tty: &File) -> Result<Self> {
        let owned_tty = tty.try_clone()?;
        let mut previous = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(tty.as_raw_fd(), &mut previous) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut mode = previous;
        mode.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
        mode.c_cc[libc::VMIN] = 1;
        mode.c_cc[libc::VTIME] = 0;
        if unsafe { libc::tcsetattr(tty.as_raw_fd(), libc::TCSANOW, &mode) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self {
            tty: owned_tty,
            previous,
        })
    }
}

impl Drop for KeyMode {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(self.tty.as_raw_fd(), libc::TCSANOW, &self.previous);
        }
    }
}

fn read_choice(tty: &mut File) -> Result<u8> {
    let _mode = KeyMode::acquire(tty)?;
    write!(
        tty,
        "[r] Review difference  [k] Keep local  [m] Use managed  [q] Cancel (default): "
    )?;
    tty.flush()?;
    let mut key = [0];
    tty.read_exact(&mut key)
        .context("read conflict review key")?;
    if key[0].is_ascii_graphic() {
        writeln!(tty, "{}", char::from(key[0]))?;
    } else {
        writeln!(tty)?;
    }
    Ok(key[0].to_ascii_lowercase())
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
    if output.stdout.is_empty() {
        writeln!(tty, "Content is identical; file metadata differs.")?;
        return Ok(());
    }
    // Sanitize before handing content to either renderer. Only renderer-generated
    // ANSI escapes may reach the terminal.
    let safe: String = String::from_utf8_lossy(&output.stdout)
        .chars()
        .flat_map(|ch| {
            if ch.is_control() && ch != '\n' && ch != '\t' {
                ch.escape_default().collect::<Vec<_>>()
            } else {
                vec![ch]
            }
        })
        .collect();
    let color = std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
        && std::env::var("TERM").as_deref() != Ok("dumb");
    if color {
        let input = directory.path().join("preview.diff");
        std::fs::write(&input, &safe)?;
        // Disable user configuration and paging: review owns the terminal and
        // must always return to the explicit conflict decision prompt.
        let status = Command::new("delta")
            .args(["--no-gitconfig", "--paging=never", "--line-numbers"])
            .stdin(File::open(input)?)
            .stdout(tty.try_clone()?)
            .stderr(Stdio::null())
            .status();
        if status.is_ok_and(|status| status.success()) {
            return Ok(());
        }
    }
    for line in safe.split_inclusive('\n') {
        let style = if line.starts_with("---") || line.starts_with("+++") {
            "\x1b[1m"
        } else if line.starts_with('+') {
            "\x1b[32m"
        } else if line.starts_with('-') {
            "\x1b[31m"
        } else if line.starts_with("@@") {
            "\x1b[36m"
        } else {
            ""
        };
        if color && !style.is_empty() {
            write!(tty, "{style}{}\x1b[0m", line.trim_end_matches('\n'))?;
            if line.ends_with('\n') {
                writeln!(tty)?;
            }
        } else {
            write!(tty, "{line}")?;
        }
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
            match read_choice(&mut tty)? {
                b'r' => {
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
                b'k' => {
                    state
                        .kept_local
                        .insert(entry.path.clone(), entry.snapshot.clone());
                    break;
                }
                b'm' => {
                    // Only this observed version is approved. apply rechecks it before
                    // backup and atomic replacement; edits made during review still fail.
                    state.kept_local.remove(&entry.path);
                    state
                        .files
                        .insert(entry.path.clone(), local.snapshot.clone());
                    break;
                }
                b'\n' | b'\r' | b'q' | 3 | 4 | 27 => {
                    bail!("conflict review cancelled; no files applied")
                }
                _ => writeln!(tty, "Choose r, k, m, or q.")?,
            }
        }
    }
    Ok(())
}
