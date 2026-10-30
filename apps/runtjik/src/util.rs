use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::{
    env, fs,
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub fn home() -> PathBuf {
    PathBuf::from(env::var_os("HOME").unwrap_or_default())
}
pub fn xdg(kind: &str) -> PathBuf {
    PathBuf::from(
        env::var_os(format!("XDG_{}_HOME", kind.to_uppercase())).unwrap_or_else(|| {
            home()
                .join(match kind {
                    "cache" => ".cache",
                    "state" => ".local/state",
                    "data" => ".local/share",
                    _ => ".config",
                })
                .into()
        }),
    )
    .join("runtjik")
}
pub fn private_dir(path: &Path) -> Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    Ok(())
}
pub fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    private_dir(path.parent().context("Missing parent")?)?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    let _ = fs::remove_file(tmp);
    result
}
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
pub fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
pub fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
pub fn which(name: &str) -> Option<PathBuf> {
    let check = |p: PathBuf| {
        if p.is_file()
            && fs::metadata(&p)
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        {
            Some(p)
        } else {
            None
        }
    };
    if name.contains('/') {
        return check(PathBuf::from(name));
    }
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .filter(|p| p.is_absolute())
        .find_map(|p| check(p.join(name)))
}
// Drain both pipes concurrently to avoid deadlocks on large discovery output.
pub fn run(args: &[String], input: Option<&str>, timeout: u64) -> Result<String> {
    let mut c = Command::new(args.first().context("Empty command")?);
    c.args(&args[1..])
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn()?;
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let a = std::thread::spawn(move || {
        let mut b = String::new();
        out.read_to_string(&mut b).map(|_| b)
    });
    let b = std::thread::spawn(move || {
        let mut b = String::new();
        err.read_to_string(&mut b).map(|_| b)
    });
    if let Some(text) = input {
        child.stdin.take().unwrap().write_all(text.as_bytes())?;
    }
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if start.elapsed() > Duration::from_secs(timeout) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Command timed out: {}", args[0]);
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let out = a.join().unwrap()?;
    let err = b.join().unwrap()?;
    if !status.success() {
        bail!("{}: {}", args[0], err.trim());
    }
    Ok(out)
}
pub fn command(args: &[&str]) -> Result<String> {
    run(
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        None,
        8,
    )
}
pub fn dispatch(mut args: Vec<String>, settings: &Value) -> Result<()> {
    if args.is_empty() || which(&args[0]).is_none() {
        bail!("Executable unavailable");
    }
    if settings["launch"]["isolate"] == true
        && env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
        && which("systemd-run").is_some()
    {
        let mut prefix = vec![
            "systemd-run".into(),
            "--user".into(),
            "--collect".into(),
            "--quiet".into(),
            "--property=Type=exec".into(),
            "--property=ExitType=cgroup".into(),
        ];
        for key in [
            "WAYLAND_DISPLAY",
            "DISPLAY",
            "HYPRLAND_INSTANCE_SIGNATURE",
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_TYPE",
            "XDG_RUNTIME_DIR",
            "PATH",
            "XDG_DATA_HOME",
            "XDG_DATA_DIRS",
        ] {
            if let Ok(v) = env::var(key) {
                prefix.push(format!("--setenv={key}={v}"));
            }
        }
        prefix.push(format!("--working-directory={}", home().display()));
        prefix.push("--".into());
        prefix.append(&mut args);
        run(&prefix, None, 8)?;
    } else {
        let mut child = Command::new(&args[0])
            .args(&args[1..])
            .current_dir(home())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    Ok(())
}

// Binary subprocess I/O for clipboard images; drain and write concurrently, bound
// output, and redirect wl-copy output because its selection owner inherits FDs.
pub fn binary(args: &[String], input: Vec<u8>, timeout: u64, discard: bool) -> Result<Vec<u8>> {
    let mut child = Command::new(args.first().context("Empty command")?)
        .args(&args[1..])
        .stdin(Stdio::piped())
        .stdout(if discard {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        if let Some(out) = stdout {
            out.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        }
        Ok(bytes)
    });
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(timeout) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Command timed out: {}", args[0]);
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let bytes = reader
        .join()
        .map_err(|_| anyhow::anyhow!("Output reader failed"))??;
    ensure_binary_status(status.success(), bytes.len())?;
    writer
        .join()
        .map_err(|_| anyhow::anyhow!("Input writer failed"))??;
    Ok(bytes)
}
fn ensure_binary_status(success: bool, length: usize) -> Result<()> {
    if !success {
        bail!("Desktop command failed");
    }
    if length > 64 * 1024 * 1024 {
        bail!("Desktop output too large");
    }
    Ok(())
}
