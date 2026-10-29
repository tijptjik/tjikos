use super::*;
use base64::Engine;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::CString,
    os::fd::{AsRawFd, FromRawFd},
    os::unix::ffi::OsStrExt,
};
fn openat(parent: i32, name: &std::ffi::OsStr, directory: bool) -> Result<fs::File> {
    let name = CString::new(name.as_bytes())?;
    let flags = libc::O_RDONLY
        | libc::O_NOFOLLOW
        | libc::O_CLOEXEC
        | libc::O_NONBLOCK
        | if directory { libc::O_DIRECTORY } else { 0 };
    let fd = unsafe { libc::openat(parent, name.as_ptr(), flags) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { fs::File::from_raw_fd(fd) })
}
fn open_root(mount: &Path, relative: &str) -> Result<fs::File> {
    if !mount.is_absolute() {
        bail!("mount must be absolute");
    }
    let mut fd = fs::File::open("/")?;
    for part in mount.components().chain(Path::new(relative).components()) {
        match part {
            std::path::Component::RootDir => (),
            std::path::Component::Normal(name) => fd = openat(fd.as_raw_fd(), name, true)?,
            _ => bail!("unsafe media root"),
        }
    }
    Ok(fd)
}
struct Journal {
    state: PathBuf,
    path: Option<PathBuf>,
    file: Option<fs::File>,
}
impl Journal {
    fn record(&mut self, path: &Path, info: &fs::Metadata, file: &fs::File) -> Result<()> {
        if self.file.is_none() {
            private_dir(&self.state)?;
            let path = self.state.join(format!("changes-{}.jsonl", stamp()));
            use std::os::unix::fs::OpenOptionsExt;
            self.file = Some(
                fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .mode(0o600)
                    .open(&path)?,
            );
            self.path = Some(path);
        }
        let mut record = serde_json::json!({"path":path,"uid":info.uid(),"gid":info.gid(),"mode":info.mode()&0o7777,"device":info.dev(),"inode":info.ino(),"size":info.len(),"mtimeNs":info.mtime()as i128*1_000_000_000+info.mtime_nsec()as i128});
        for key in ["system.posix_acl_access", "system.posix_acl_default"] {
            let name = CString::new(key)?;
            let size = unsafe {
                libc::fgetxattr(file.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0)
            };
            if size < 0 {
                let error = std::io::Error::last_os_error();
                if ![Some(libc::ENODATA), Some(libc::ENOTSUP)].contains(&error.raw_os_error()) {
                    return Err(error.into());
                }
                continue;
            }
            let mut bytes = vec![0u8; size as usize];
            let read = unsafe {
                libc::fgetxattr(
                    file.as_raw_fd(),
                    name.as_ptr(),
                    bytes.as_mut_ptr().cast(),
                    bytes.len(),
                )
            };
            if read < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            bytes.truncate(read as usize);
            record[key] =
                serde_json::json!(base64::engine::general_purpose::STANDARD.encode(bytes));
        }
        let output = self.file.as_mut().unwrap();
        serde_json::to_writer(&mut *output, &record)?;
        output.write_all(b"\n")?;
        output.flush()?;
        output.sync_all()?;
        Ok(())
    }
}
fn update(
    file: &fs::File,
    path: &Path,
    device: u64,
    gid: u32,
    apply: bool,
    journal: &mut Journal,
    counts: &mut BTreeMap<String, u64>,
) -> Result<()> {
    let current = file.metadata()?;
    if current.dev() != device {
        bail!("nested filesystem refused: {}", path.display());
    }
    if !current.is_dir() && !current.is_file() {
        *counts.entry("skippedSpecial".into()).or_default() += 1;
        return Ok(());
    }
    let kind = if current.is_dir() {
        "directories"
    } else {
        "files"
    };
    *counts.entry(kind.into()).or_default() += 1;
    let mode = (current.mode() & 0o7777) | if current.is_dir() { 0o2070 } else { 0o060 };
    if current.gid() == gid && (current.mode() & 0o7777) == mode {
        return Ok(());
    }
    *counts.entry("changed".into()).or_default() += 1;
    *counts
        .entry(
            if current.is_dir() {
                "changedDirectories"
            } else {
                "changedFiles"
            }
            .into(),
        )
        .or_default() += 1;
    if apply {
        journal.record(path, &current, file)?;
        if current.gid() != gid && unsafe { libc::fchown(file.as_raw_fd(), u32::MAX, gid) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        let after = file.metadata()?;
        if (after.uid(), after.gid(), after.mode() & 0o7777) != (current.uid(), gid, mode) {
            bail!("permission verification failed");
        }
    }
    Ok(())
}
fn walk(
    file: &fs::File,
    path: &Path,
    device: u64,
    gid: u32,
    apply: bool,
    journal: &mut Journal,
    counts: &mut BTreeMap<String, u64>,
) -> Result<()> {
    update(file, path, device, gid, apply, journal, counts)?;
    for item in fs::read_dir(format!("/proc/self/fd/{}", file.as_raw_fd()))? {
        let item = item?;
        let name = item.file_name();
        let kind = item.file_type()?;
        if !kind.is_file() && !kind.is_dir() {
            *counts.entry("skippedSpecial".into()).or_default() += 1;
            continue;
        }
        let child = match openat(file.as_raw_fd(), &name, kind.is_dir()) {
            Ok(f) => f,
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                *counts.entry("vanishedDuringScan".into()).or_default() += 1;
                continue;
            }
            Err(e) => return Err(e),
        };
        if child.metadata()?.is_dir() {
            walk(
                &child,
                &path.join(name),
                device,
                gid,
                apply,
                journal,
                counts,
            )?;
        } else {
            update(
                &child,
                &path.join(name),
                device,
                gid,
                apply,
                journal,
                counts,
            )?;
        }
    }
    Ok(())
}
pub fn run(policy: &Value, apply: bool) -> Result<i32> {
    let p = &policy["permissions"];
    let mut locks = Vec::new();
    for name in p["locks"].as_array().context("permission locks missing")? {
        match crate::process::Lock::acquire(Path::new(name.as_str().context("lock path missing")?))
        {
            Ok(lock) => locks.push(lock),
            Err(error) => {
                if error.to_string().starts_with("workflow/repository is busy") {
                    println!(
                        "Media permissions deferred: storage maintenance or backup is running."
                    );
                    return Ok(75);
                }
                return Err(error);
            }
        }
    }
    let rules = read(Path::new(text(p, "policy")?))?;
    let gid = rules["gid"]
        .as_u64()
        .filter(|n| *n <= u32::MAX as u64)
        .context("invalid media group identity")? as u32;
    let group = command(&["getent", "group", text(&rules, "group")?])?;
    if group.trim().split(':').nth(2) != Some(gid.to_string().as_str()) {
        bail!("media group identity changed");
    }
    let roots: BTreeSet<_> = rules["roots"]
        .as_array()
        .context("media roots missing")?
        .iter()
        .map(|v| v.as_str().context("media root must be a string"))
        .collect::<Result<_>>()?;
    let expected: BTreeSet<_> = p["allowed_roots"]
        .as_array()
        .context("allowed roots missing")?
        .iter()
        .map(|v| v.as_str().context("allowed root must be a string"))
        .collect::<Result<_>>()?;
    if roots != expected {
        bail!("unexpected media permission scope");
    }
    let config = storage::config(policy)?;
    storage::check_mounts(policy, &config, false)?;
    let mut opened = Vec::new();
    let mut seen = BTreeSet::new();
    for row in config["layout"]
        .as_array()
        .context("layout missing")?
        .iter()
        .filter(|r| r[0] == "data")
    {
        let mount = Path::new(row[2].as_str().context("mount path missing")?);
        for relative in &roots {
            let file = match open_root(mount, relative) {
                Ok(f) => f,
                Err(e)
                    if e.downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                {
                    continue
                }
                Err(e) => return Err(e),
            };
            if file.metadata()?.dev() != fs::metadata(mount)?.dev() {
                bail!("nested filesystem refused");
            }
            seen.insert(*relative);
            opened.push((mount.join(relative), file));
        }
    }
    if seen != roots {
        bail!("missing declared media roots");
    }
    let state = PathBuf::from(text(p, "state")?);
    let mut journal = Journal {
        state: state.clone(),
        path: None,
        file: None,
    };
    let mut counts = BTreeMap::new();
    for (path, file) in opened {
        walk(
            &file,
            &path,
            file.metadata()?.dev(),
            gid,
            apply,
            &mut journal,
            &mut counts,
        )?;
    }
    let mut result = serde_json::to_value(counts)?;
    result["apply"] = serde_json::json!(apply);
    result["completedAt"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
    result["journal"] = serde_json::json!(journal.path);
    if apply {
        private_dir(&state)?;
        receipt(&state.join("last-success.json"), &result)?;
    }
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audit_changes_nothing_and_apply_journals_identity_without_following_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("media");
        fs::create_dir(&root).unwrap();
        let path = root.join("movie");
        fs::write(&path, b"media bytes").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let outside = temp.path().join("outside");
        fs::write(&outside, b"private").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
        let before = fs::metadata(&path).unwrap();
        let fd = open_root(&root, "").unwrap();
        let mut journal = Journal {
            state: temp.path().join("journal"),
            file: None,
            path: None,
        };
        let mut counts = BTreeMap::new();
        walk(
            &fd,
            &root,
            before.dev(),
            before.gid(),
            false,
            &mut journal,
            &mut counts,
        )
        .unwrap();
        assert!(!journal.state.exists());
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        walk(
            &fd,
            &root,
            before.dev(),
            before.gid(),
            true,
            &mut journal,
            &mut counts,
        )
        .unwrap();
        let after = fs::metadata(&path).unwrap();
        assert_eq!(
            (after.uid(), after.ino(), after.mtime(), after.mtime_nsec()),
            (
                before.uid(),
                before.ino(),
                before.mtime(),
                before.mtime_nsec()
            )
        );
        assert_eq!(after.mode() & 0o777, 0o660);
        assert_eq!(fs::read(&path).unwrap(), b"media bytes");
        assert_eq!(fs::metadata(&outside).unwrap().mode() & 0o777, 0o600);
        let evidence = fs::read_to_string(journal.path.unwrap()).unwrap();
        assert!(evidence
            .lines()
            .any(|line| serde_json::from_str::<Value>(line).unwrap()["inode"] == before.ino()));
        assert!(open_root(&root, "../outside").is_err());
        assert!(open_root(&root, "escape").is_err());
    }
}
