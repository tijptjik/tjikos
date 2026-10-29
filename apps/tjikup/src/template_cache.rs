//! Private, content-addressed cache for explicitly non-secret maintenance scripts.
use crate::config::Render;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub struct Cache {
    directory: PathBuf,
}
impl Cache {
    pub fn new(
        batch: &Render,
        fingerprint: Option<&str>,
        sources: &[Vec<u8>],
    ) -> Result<Option<Self>> {
        if !batch.cache {
            return Ok(None);
        }
        let Some(fingerprint) = fingerprint else {
            return Ok(None);
        };
        let home = std::env::var_os("HOME").context("HOME required for template cache")?;
        let root = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(home).join(".cache"));
        let root = root.join("tjikup/templates");
        // Walk the cache tree without accepting symlink directory components.
        let mut current = PathBuf::new();
        for part in root.components() {
            current.push(part);
            match fs::symlink_metadata(&current) {
                Ok(m) if m.is_dir() => (),
                Ok(_) => bail!("template cache path contains a non-directory"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    fs::DirBuilder::new().mode(0o700).create(&current)?
                }
                Err(e) => return Err(e.into()),
            }
        }
        let meta = fs::metadata(&root)?;
        if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
            bail!("template cache must be private and owned by this user");
        }
        // The executable hash invalidates changes to rendering semantics as well
        // as dependency upgrades, without relying on a manually bumped version.
        let renderer = hash(&fs::read(std::env::current_exe()?)?);
        let key = hash(&serde_json::to_vec(&(
            "native-script-cache-v1",
            renderer,
            fingerprint,
            &batch.templates,
            sources,
        ))?);
        Ok(Some(Self {
            directory: root.join(key),
        }))
    }
    pub fn restore(&self, batch: &Render) -> Option<Vec<Vec<u8>>> {
        let meta = self.directory.symlink_metadata().ok()?;
        if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
            return None;
        }
        let hashes: Vec<String> =
            serde_json::from_slice(&private_read(&self.directory.join("hashes.json")).ok()?)
                .ok()?;
        if hashes.len() != batch.templates.len() {
            return None;
        }
        batch
            .templates
            .iter()
            .zip(hashes)
            .map(|(t, expected)| {
                let bytes = private_read(&self.directory.join(&t.name)).ok()?;
                (hash(&bytes) == expected).then_some(bytes)
            })
            .collect()
    }
    pub fn store(&self, batch: &Render, scripts: &[Vec<u8>]) -> Result<()> {
        let stage = tempfile::tempdir_in(self.directory.parent().unwrap())?;
        fs::set_permissions(stage.path(), fs::Permissions::from_mode(0o700))?;
        let mut hashes = Vec::new();
        for (template, bytes) in batch.templates.iter().zip(scripts) {
            let mut file = crate::process::private_file(&stage.path().join(&template.name))?;
            file.write_all(bytes)?;
            file.sync_all()?;
            hashes.push(hash(bytes));
        }
        let mut file = crate::process::private_file(&stage.path().join("hashes.json"))?;
        file.write_all(&serde_json::to_vec(&hashes)?)?;
        file.sync_all()?;
        // An existing entry can be a concurrent successful writer. Never replace
        // arbitrary existing paths or recursively remove a corrupted cache.
        match fs::rename(stage.path(), &self.directory) {
            Ok(()) => (),
            Err(_) if self.directory.exists() => (),
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }
}
fn private_read(path: &Path) -> Result<Vec<u8>> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        bail!("invalid template cache file");
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}
