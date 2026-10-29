//! A display-only spinner: never reads terminal input or sends capability probes.
use anyhow::{bail, Result};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    time::{Duration, Instant},
};

fn transfer(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    file.seek(SeekFrom::Start(file.metadata()?.len().saturating_sub(8192)))?;
    let mut bytes = Vec::new();
    file.take(8192).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(text
        .split(['\r', '\n'])
        .rev()
        .find_map(|line| {
            let line = line.strip_prefix("remote: ").unwrap_or(line);
            [
                "Receiving objects:",
                "Resolving deltas:",
                "Updating files:",
                "Compressing objects:",
                "Counting objects:",
            ]
            .iter()
            .find_map(|prefix| {
                let rest = line.strip_prefix(prefix)?.trim();
                let percent = rest.split_once('%')?.0.parse::<u8>().ok()?;
                (percent <= 100).then(|| {
                    format!(
                        "{} {percent}%",
                        prefix.replace("Receiving objects:", "Download:")
                    )
                })
            })
        })
        .unwrap_or_default())
}
pub fn cli(args: &[String]) -> Result<i32> {
    if args.len() != 4 {
        bail!("progress LOG STATUS PARENT_PID TITLE");
    }
    let pid: i32 = args[2].parse()?;
    if pid <= 1 {
        bail!("invalid parent process");
    }
    let start = Instant::now();
    let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let mut output = std::io::stdout().lock();
    let mut frame = 0;
    while fs::metadata(&args[1])?.len() == 0 && unsafe { libc::kill(pid, 0) } == 0 {
        let detail = transfer(Path::new(&args[0]))?;
        write!(
            output,
            "\r\x1b[2K{} {}  {}s{}{}",
            frames[frame % frames.len()],
            args[3],
            start.elapsed().as_secs(),
            if detail.is_empty() { "" } else { " | " },
            detail
        )?;
        output.flush()?;
        frame += 1;
        std::thread::sleep(Duration::from_millis(100));
    }
    write!(output, "\r\x1b[2K")?;
    output.flush()?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_never_relays_arbitrary_log_text() {
        let f = tempfile::NamedTempFile::new().unwrap();
        fs::write(
            f.path(),
            "secret\nremote: Receiving objects: 12% private-token\rpassword",
        )
        .unwrap();
        assert_eq!(transfer(f.path()).unwrap(), "Download: 12%");
        fs::write(f.path(), "Receiving objects: 200%\n\x1b]0;secret\x07").unwrap();
        assert_eq!(transfer(f.path()).unwrap(), "");
    }
}
