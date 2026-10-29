//! Linux file operations anchored to directory descriptors. Parent symlinks are
//! rejected and never traversed, including when a directory is renamed mid-apply.
use super::model::{hash, Desired, Snapshot};
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

fn string(bytes: &[u8]) -> Result<CString> {
    Ok(CString::new(bytes)?)
}
fn checked(value: i32) -> Result<i32> {
    if value < 0 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(value)
    }
}
fn directory(fd: i32, name: &CStr) -> Result<File> {
    let fd = checked(unsafe {
        libc::openat(
            fd,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    })?;
    Ok(unsafe { File::from_raw_fd(fd) })
}

pub struct Parent {
    dir: File,
    name: CString,
}

pub fn parent(root: &Path, path: &Path, create: bool) -> Result<Option<Parent>> {
    let mut dir = directory(libc::AT_FDCWD, &string(root.as_os_str().as_bytes())?)?;
    let parts: Vec<_> = path.strip_prefix("/")?.components().collect();
    for part in &parts[..parts.len() - 1] {
        let name = string(part.as_os_str().as_bytes())?;
        match directory(dir.as_raw_fd(), &name) {
            Ok(next) => dir = next,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                if !create {
                    return Ok(None);
                }
                let code = unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), 0o755) };
                if code < 0
                    && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
                {
                    checked(code)?;
                }
                dir.sync_all()?;
                dir = directory(dir.as_raw_fd(), &name)?;
            }
            Err(error) => {
                return Err(error.context(format!(
                    "unsafe or inaccessible parent of {}",
                    path.display()
                )))
            }
        }
    }
    Ok(Some(Parent {
        dir,
        name: string(parts.last().unwrap().as_os_str().as_bytes())?,
    }))
}

pub struct Existing {
    pub snapshot: Snapshot,
    pub bytes: Vec<u8>,
    pub xattrs: BTreeMap<String, Vec<u8>>,
}
impl Existing {
    fn absent() -> Self {
        Self {
            snapshot: Snapshot::Absent,
            bytes: Vec::new(),
            xattrs: BTreeMap::new(),
        }
    }
}

fn xattrs(file: &File) -> Result<BTreeMap<String, Vec<u8>>> {
    let fd = file.as_raw_fd();
    let size = unsafe { libc::flistxattr(fd, std::ptr::null_mut(), 0) };
    if size < 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ENOTSUP) {
            return Ok(BTreeMap::new());
        }
        return Err(err.into());
    }
    let mut names = vec![0u8; size as usize];
    let got = unsafe { libc::flistxattr(fd, names.as_mut_ptr().cast(), names.len()) };
    if got < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    names.truncate(got as usize);
    let mut result = BTreeMap::new();
    for name in names.split(|c| *c == 0).filter(|s| !s.is_empty()) {
        let c_name = string(name)?;
        let len = unsafe { libc::fgetxattr(fd, c_name.as_ptr(), std::ptr::null_mut(), 0) };
        if len < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut value = vec![0u8; len as usize];
        let len =
            unsafe { libc::fgetxattr(fd, c_name.as_ptr(), value.as_mut_ptr().cast(), value.len()) };
        if len < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        value.truncate(len as usize);
        result.insert(String::from_utf8(name.to_vec())?, value);
    }
    Ok(result)
}

impl Parent {
    pub fn read(&self) -> Result<Existing> {
        let fd = self.dir.as_raw_fd();
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        let code = unsafe {
            libc::fstatat(
                fd,
                self.name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if code < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::NotFound {
                return Ok(Existing::absent());
            }
            return Err(err.into());
        }
        let stat = unsafe { stat.assume_init() };
        match stat.st_mode & libc::S_IFMT {
            libc::S_IFDIR => {
                let file = directory(fd, &self.name)?;
                let meta = file.metadata()?;
                if meta.ino() != stat.st_ino || meta.dev() != stat.st_dev {
                    bail!("destination changed while reading");
                }
                Ok(Existing {
                    snapshot: Snapshot::Directory {
                        mode: meta.mode() & 0o7777,
                        uid: meta.uid(),
                        gid: meta.gid(),
                    },
                    bytes: Vec::new(),
                    xattrs: xattrs(&file)?,
                })
            }
            libc::S_IFREG => {
                let raw = checked(unsafe {
                    libc::openat(
                        fd,
                        self.name.as_ptr(),
                        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                    )
                })?;
                let mut file = unsafe { File::from_raw_fd(raw) };
                let meta = file.metadata()?;
                if !meta.is_file() || meta.ino() != stat.st_ino || meta.dev() != stat.st_dev {
                    bail!("destination changed while reading");
                }
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                Ok(Existing {
                    snapshot: Snapshot::File {
                        sha256: hash(&bytes),
                        mode: meta.mode() & 0o7777,
                        uid: meta.uid(),
                        gid: meta.gid(),
                    },
                    bytes,
                    xattrs: xattrs(&file)?,
                })
            }
            libc::S_IFLNK => {
                let mut target = vec![0u8; 65536];
                let len = unsafe {
                    libc::readlinkat(
                        fd,
                        self.name.as_ptr(),
                        target.as_mut_ptr().cast(),
                        target.len(),
                    )
                };
                if len < 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                if len as usize == target.len() {
                    bail!("symlink target too long");
                }
                target.truncate(len as usize);
                Ok(Existing {
                    snapshot: Snapshot::Symlink {
                        target: String::from_utf8(target)?,
                        uid: stat.st_uid,
                        gid: stat.st_gid,
                    },
                    bytes: Vec::new(),
                    xattrs: BTreeMap::new(),
                })
            }
            _ => bail!("refusing to replace a directory or special file"),
        }
    }

    pub fn write(&self, desired: &Desired, previous: &Existing, live_root: bool) -> Result<()> {
        if self.read()?.snapshot != previous.snapshot {
            bail!("destination changed after planning");
        }
        if desired.snapshot == Snapshot::Absent {
            if previous.snapshot != Snapshot::Absent {
                let flags = if matches!(previous.snapshot, Snapshot::Directory { .. }) {
                    libc::AT_REMOVEDIR
                } else {
                    0
                };
                checked(unsafe {
                    libc::unlinkat(self.dir.as_raw_fd(), self.name.as_ptr(), flags)
                })?;
                self.dir.sync_all()?;
            }
            return Ok(());
        }
        if let Snapshot::Directory { mode, uid, gid } = &desired.snapshot {
            if previous.snapshot == Snapshot::Absent {
                checked(unsafe { libc::mkdirat(self.dir.as_raw_fd(), self.name.as_ptr(), *mode) })?;
            } else if !matches!(previous.snapshot, Snapshot::Directory { .. }) {
                bail!("refusing to replace a file with a directory");
            }
            let file = directory(self.dir.as_raw_fd(), &self.name)?;
            let meta = file.metadata()?;
            if meta.uid() != *uid || meta.gid() != *gid {
                checked(unsafe { libc::fchown(file.as_raw_fd(), *uid, *gid) })?;
            }
            for (key, value) in &previous.xattrs {
                checked(unsafe {
                    libc::fsetxattr(
                        file.as_raw_fd(),
                        string(key.as_bytes())?.as_ptr(),
                        value.as_ptr().cast(),
                        value.len(),
                        0,
                    )
                })?;
            }
            file.set_permissions(std::fs::Permissions::from_mode(*mode))?;
            file.sync_all()?;
            self.dir.sync_all()?;
            if self.read()?.snapshot != desired.snapshot {
                bail!("directory failed post-write verification");
            }
            return Ok(());
        }
        if matches!(previous.snapshot, Snapshot::Directory { .. }) {
            bail!("refusing to replace a directory with a file");
        }
        let proc_dir = format!("/proc/self/fd/{}", self.dir.as_raw_fd());
        let mut staged = tempfile::Builder::new()
            .prefix(".tjikup-")
            .tempfile_in(&proc_dir)?;
        let name = string(staged.path().file_name().unwrap().as_bytes())?;
        match &desired.snapshot {
            Snapshot::File { mode, uid, gid, .. } => {
                staged.write_all(&desired.bytes)?;
                let fd = staged.as_raw_fd();
                let meta = staged.as_file().metadata()?;
                if meta.uid() != *uid || meta.gid() != *gid {
                    checked(unsafe { libc::fchown(fd, *uid, *gid) })?;
                }
                // Preserve existing ACLs and SELinux labels; never silently drop xattrs.
                for (key, value) in &previous.xattrs {
                    checked(unsafe {
                        libc::fsetxattr(
                            fd,
                            string(key.as_bytes())?.as_ptr(),
                            value.as_ptr().cast(),
                            value.len(),
                            0,
                        )
                    })?;
                }
                // chmod last: an explicitly changed mode must also update any ACL mask.
                staged
                    .as_file()
                    .set_permissions(std::fs::Permissions::from_mode(*mode))?;
                if live_root
                    && previous.snapshot == Snapshot::Absent
                    && Path::new("/sys/fs/selinux/enforce").exists()
                {
                    let output = std::process::Command::new("matchpathcon")
                        .arg("-n")
                        .arg(&desired.path)
                        .output()
                        .context("resolve new file SELinux label")?;
                    if !output.status.success() {
                        bail!(
                            "cannot resolve SELinux label for {}",
                            desired.path.display()
                        );
                    }
                    let context = String::from_utf8(output.stdout)?;
                    // No matching policy rule means keep the label assigned by
                    // the kernel at creation; <<none>> is not a security context.
                    if context.trim() != "<<none>>" {
                        let label = string(context.trim().as_bytes())?;
                        checked(unsafe {
                            libc::fsetxattr(
                                fd,
                                c"security.selinux".as_ptr(),
                                label.as_ptr().cast(),
                                label.as_bytes_with_nul().len(),
                                0,
                            )
                        })?;
                    }
                }
                staged.as_file().sync_all()?;
            }
            Snapshot::Symlink { target, uid, gid } => {
                checked(unsafe { libc::unlinkat(self.dir.as_raw_fd(), name.as_ptr(), 0) })?;
                checked(unsafe {
                    libc::symlinkat(
                        string(target.as_bytes())?.as_ptr(),
                        self.dir.as_raw_fd(),
                        name.as_ptr(),
                    )
                })?;
                checked(unsafe {
                    libc::fchownat(
                        self.dir.as_raw_fd(),
                        name.as_ptr(),
                        *uid,
                        *gid,
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                })?;
            }
            Snapshot::Absent | Snapshot::Directory { .. } => unreachable!(),
        }
        if self.read()?.snapshot != previous.snapshot {
            bail!("destination changed before replacement");
        }
        checked(unsafe {
            libc::renameat(
                self.dir.as_raw_fd(),
                name.as_ptr(),
                self.dir.as_raw_fd(),
                self.name.as_ptr(),
            )
        })?;
        self.dir.sync_all()?;
        if self.read()?.snapshot != desired.snapshot {
            bail!("destination failed post-write verification");
        }
        Ok(())
    }
}

pub fn read(root: &Path, path: &Path) -> Result<Existing> {
    match parent(root, path, false)? {
        Some(parent) => parent.read(),
        None => Ok(Existing::absent()),
    }
}
