//! Authenticated ciphertext only. The encryption key lives in Secret Service.
use anyhow::{bail, ensure, Context, Result};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
            process::CommandExt,
        },
    },
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub directory: Option<PathBuf>,
    pub keyring_command: Vec<String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            directory: None,
            keyring_command: vec!["secret-tool".into()],
        }
    }
}
pub struct Cache {
    root: PathBuf,
    key: [u8; 32],
    scope: String,
}
#[derive(Serialize, Deserialize)]
struct Entry {
    value: String,
    refresh_run: Option<String>,
}
const HEADER: &[u8] = b"TJBC\x01";
const LIMIT: u64 = 1024 * 1024 + 4096;
pub fn random_hex() -> Result<String> {
    let mut key = [0u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut key)?;
    Ok(key.iter().map(|v| format!("{v:02x}")).collect())
}
fn decode_key(value: &[u8]) -> Result<[u8; 32]> {
    let text = std::str::from_utf8(value)?.trim();
    ensure!(text.len() == 64, "Invalid cache key in secret service");
    let mut key = [0; 32];
    for (i, pair) in text.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        key[i] = u8::from_str_radix(std::str::from_utf8(pair)?, 16)
            .context("Invalid cache key in secret service")?;
    }
    Ok(key)
}
fn safe_path(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute() && !path.components().any(|c| matches!(c, Component::ParentDir)),
        "Credential cache path must be absolute without traversal"
    );
    for part in path.ancestors() {
        match fs::symlink_metadata(part) {
            Ok(m) => ensure!(
                !m.file_type().is_symlink(),
                "Credential cache path contains a symlink"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn private_file(path: &Path, create: bool) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(create)
        .create(create)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let m = file.metadata()?;
    ensure!(
        m.is_file()
            && m.uid() == unsafe { libc::geteuid() }
            && m.mode() & 0o077 == 0
            && m.nlink() == 1,
        "Credential cache file is not private and owner-controlled"
    );
    Ok(file)
}
pub struct Guard(File);
impl Guard {
    fn acquire(path: &Path) -> Result<Self> {
        let file = private_file(path, true)?;
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(Self(file));
            }
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::EWOULDBLOCK) {
                return Err(e.into());
            }
            ensure!(Instant::now() < deadline, "Credential cache is busy");
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
fn keyring(settings: &Settings, args: &[&str], input: Option<&[u8]>) -> Result<(i32, Vec<u8>)> {
    ensure!(
        !settings.keyring_command.is_empty(),
        "Secret-service command is empty"
    );
    let mut cmd = Command::new(&settings.keyring_command[0]);
    cmd.args(&settings.keyring_command[1..])
        .args(args)
        .env_remove("BWS_ACCESS_TOKEN");
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .context("Secret service unavailable")?;
    if let Some(data) = input {
        let result = child.stdin.take().unwrap().write_all(data);
        if result.is_err() {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Secret-service input failed");
        }
    } else {
        child.stdin.take();
    }
    let mut stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let result = stdout
            .by_ref()
            .take(4097)
            .read_to_end(&mut out)
            .map(|_| out);
        let _ = tx.send(result);
    });
    let started = Instant::now();
    let result = (|| loop {
        if let Some(status) = child.try_wait()? {
            let data = rx
                .recv_timeout(Duration::from_secs(5).saturating_sub(started.elapsed()))
                .context("Secret-service response timed out")??;
            ensure!(data.len() <= 4096, "Secret-service response too large");
            return Ok((status.code().unwrap_or(-1), data));
        }
        ensure!(
            started.elapsed() < Duration::from_secs(5),
            "Secret service is locked or unavailable"
        );
        std::thread::sleep(Duration::from_millis(10));
    })();
    if result.is_err() {
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.wait();
    }
    result
}
impl Cache {
    pub fn open(settings: &Settings, scope: &str) -> Result<Self> {
        let root = match &settings.directory {
            Some(p) => p.clone(),
            None => {
                let base = std::env::var_os("XDG_CACHE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
                    });
                base.join("tjikup/credentials")
            }
        };
        safe_path(&root)?;
        if !root.exists() {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&root)?;
        }
        let info = fs::symlink_metadata(&root)?;
        ensure!(
            info.is_dir() && info.uid() == unsafe { libc::geteuid() } && info.mode() & 0o077 == 0,
            "Credential cache directory must be owner-only"
        );
        let _guard = Guard::acquire(&root.join("key.lock"))?;
        let attrs = [
            "application",
            "tjikup",
            "purpose",
            "bitwarden-cache",
            "version",
            "1",
        ];
        let mut lookup = vec!["lookup"];
        lookup.extend(attrs);
        let (code, mut value) = keyring(settings, &lookup, None)?;
        if code != 0 || value.is_empty() {
            ensure!(code == 0 || code == 1, "Secret-service lookup failed");
            let generated = random_hex()?;
            let mut store = vec!["store", "--label=Tjikup encrypted credential cache"];
            store.extend(attrs);
            ensure!(
                keyring(settings, &store, Some(generated.as_bytes()))?.0 == 0,
                "Cannot store encryption key in secret service"
            );
            let (code, stored) = keyring(settings, &lookup, None)?;
            ensure!(
                code == 0 && stored.strip_suffix(b"\n").unwrap_or(&stored) == generated.as_bytes(),
                "Secret service did not retain cache key"
            );
            value = stored;
        }
        Ok(Self {
            root,
            key: decode_key(&value)?,
            scope: scope.into(),
        })
    }
    fn name(&self, id: &str) -> String {
        format!("{:x}", Sha256::digest(self.aad(id)))
    }
    fn aad(&self, id: &str) -> Vec<u8> {
        format!("tjikup-bitwarden-cache-v1\0{}\0{id}", self.scope).into_bytes()
    }
    pub fn lock(&self, id: &str) -> Result<Guard> {
        Guard::acquire(&self.root.join(format!("{}.lock", self.name(id))))
    }
    pub fn get(&self, id: &str, refresh: Option<&str>) -> Result<Option<String>> {
        let path = self.root.join(format!("{}.enc", self.name(id)));
        let file = match private_file(&path, false) {
            Ok(f) => f,
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(None)
            }
            Err(e) => return Err(e),
        };
        let mut data = Vec::new();
        file.take(LIMIT + 1).read_to_end(&mut data)?;
        if data.len() as u64 > LIMIT || data.len() < HEADER.len() + 12 || !data.starts_with(HEADER)
        {
            return Ok(None);
        }
        let cipher = ChaCha20Poly1305::new((&self.key).into());
        let bytes = match cipher.decrypt(
            Nonce::from_slice(&data[HEADER.len()..HEADER.len() + 12]),
            Payload {
                msg: &data[HEADER.len() + 12..],
                aad: &self.aad(id),
            },
        ) {
            Ok(b) => b,
            Err(_) => return Ok(None),
        };
        let entry: Entry = match serde_json::from_slice(&bytes) {
            Ok(e) => e,
            Err(_) => return Ok(None),
        };
        if refresh.is_some_and(|run| entry.refresh_run.as_deref() != Some(run)) {
            return Ok(None);
        }
        Ok(Some(entry.value))
    }
    pub fn put(&self, id: &str, value: &str, refresh: Option<&str>) -> Result<()> {
        let mut nonce = [0; 12];
        File::open("/dev/urandom")?.read_exact(&mut nonce)?;
        let plain = serde_json::to_vec(&Entry {
            value: value.into(),
            refresh_run: refresh.map(str::to_owned),
        })?;
        let cipher = ChaCha20Poly1305::new((&self.key).into());
        let encrypted = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: &self.aad(id),
                },
            )
            .map_err(|_| anyhow::anyhow!("Credential encryption failed"))?;
        let mut temp = tempfile::NamedTempFile::new_in(&self.root)?;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        temp.write_all(HEADER)?;
        temp.write_all(&nonce)?;
        temp.write_all(&encrypted)?;
        temp.as_file().sync_all()?;
        temp.persist(self.root.join(format!("{}.enc", self.name(id))))?;
        File::open(&self.root)?.sync_all()?;
        Ok(())
    }
}
