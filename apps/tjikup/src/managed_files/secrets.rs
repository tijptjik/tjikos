//! Bitwarden values use an optional encrypted local cache and in-memory render cache.
use super::credential_cache::{Cache, Settings};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex, OnceLock,
};
use std::time::{Duration, Instant};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bitwarden {
    #[serde(default = "default_command")]
    pub command: Vec<String>,
    pub access_token_file: Option<String>,
    pub cache: Option<Settings>,
}
fn default_command() -> Vec<String> {
    vec!["bws".into()]
}

pub struct Secrets {
    config: Bitwarden,
    cache: Mutex<BTreeMap<String, String>>,
    pub used: AtomicBool,
    requests: AtomicU64,
    request_millis: AtomicU64,
    cache_hits: AtomicU64,
    persistent: OnceLock<Option<Cache>>,
    refresh_run: String,
}
impl Secrets {
    pub fn new(config: Bitwarden) -> Result<Self> {
        if config.command.is_empty() || config.command[0].is_empty() {
            bail!("Bitwarden command is empty");
        }
        Ok(Self {
            config,
            cache: Mutex::new(BTreeMap::new()),
            used: AtomicBool::new(false),
            requests: AtomicU64::new(0),
            request_millis: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            persistent: OnceLock::new(),
            refresh_run: std::env::var("TJIKUP_SECRET_REFRESH_RUN")
                .unwrap_or(super::credential_cache::random_hex()?),
        })
    }
    fn token(&self) -> Result<String> {
        if let Ok(token) = std::env::var("BWS_ACCESS_TOKEN") {
            if !token.trim().is_empty() {
                return Ok(token);
            }
        }
        let path = self.config.access_token_file.as_ref().context(
            "Bitwarden authentication missing: set BWS_ACCESS_TOKEN or access_token_file",
        )?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(Path::new(path))
            .context("cannot read Bitwarden access-token file")?;
        let info = file.metadata()?;
        if !info.is_file() || info.uid() != unsafe { libc::geteuid() } || info.mode() & 0o077 != 0 {
            bail!("Bitwarden access-token file must be owned by the invoking user with mode 0600");
        }
        let mut data = String::new();
        file.take(65537)
            .read_to_string(&mut data)
            .context("invalid Bitwarden credential file")?;
        if data.len() > 65536 {
            bail!("Bitwarden credential file is too large");
        }
        let tokens: Vec<_> = data
            .lines()
            .filter_map(|line| line.strip_prefix("BWS_ACCESS_TOKEN="))
            .collect();
        if tokens.len() != 1 || tokens[0].trim().is_empty() {
            bail!("Bitwarden credential file requires one BWS_ACCESS_TOKEN entry");
        }
        Ok(tokens[0].trim().to_owned())
    }
    pub fn get(&self, id: &str) -> Result<String> {
        self.used.store(true, Ordering::SeqCst);
        if id.len() != 36
            || !id.bytes().enumerate().all(|(i, c)| {
                if [8, 13, 18, 23].contains(&i) {
                    c == b'-'
                } else {
                    c.is_ascii_hexdigit()
                }
            })
        {
            bail!("Bitwarden secret identifier must be a UUID");
        }
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| anyhow::anyhow!("Bitwarden cache unavailable"))?;
        if let Some(value) = cache.get(id) {
            return Ok(value.clone());
        }
        let token = self.token()?;
        let refresh = std::env::var("TJIKUP_REFRESH_SECRETS").unwrap_or_default();
        let refresh = (refresh == "all" || refresh == "1" || refresh.split(',').any(|v| v == id))
            .then_some(self.refresh_run.as_str());
        let persistent = self.persistent.get_or_init(|| {
            self.config.cache.as_ref().and_then(|settings| {
                let scope = format!("{:x}", Sha256::digest(serde_json::to_vec(&(&self.config.command, &token)).expect("serializable scope")));
                match Cache::open(settings, &scope) {
                    Ok(cache) => Some(cache),
                    Err(_) => {
                        eprintln!("Credential cache unavailable; using Bitwarden directly (no plaintext cache)");
                        None
                    }
                }
            })
        });
        // Hold one per-secret lock across miss/fetch/save to avoid refresh stampedes.
        let _guard = persistent.as_ref().map(|c| c.lock(id)).transpose()?;
        if let Some(persistent) = persistent {
            if let Some(value) = persistent.get(id, refresh)? {
                self.cache_hits.fetch_add(1, Ordering::Relaxed);
                cache.insert(id.into(), value.clone());
                return Ok(value);
            }
        }
        let mut child = Command::new(&self.config.command[0])
            .args(&self.config.command[1..])
            .args(["secret", "get", id, "--output", "json", "--color", "no"])
            .env("BWS_ACCESS_TOKEN", token)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .context("cannot start Bitwarden Secrets Manager CLI")?;
        let stdout = child.stdout.take().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut data = Vec::new();
            let result = stdout
                .take(1024 * 1024 + 1)
                .read_to_end(&mut data)
                .map(|_| data);
            let _ = sender.send(result);
        });
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() > Duration::from_secs(30) {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.wait();
                bail!("Bitwarden request timed out");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        // A descendant can keep stdout open even after the CLI exits. Bound the
        // complete response read by the same deadline as the provider process.
        let output = match receiver
            .recv_timeout(Duration::from_secs(30).saturating_sub(started.elapsed()))
        {
            Ok(output) => output?,
            Err(_) => {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                bail!("Bitwarden response timed out");
            }
        };
        self.requests.fetch_add(1, Ordering::Relaxed);
        self.request_millis
            .fetch_add(started.elapsed().as_millis() as u64, Ordering::Relaxed);
        if !status.success() {
            bail!("Bitwarden secret lookup failed; verify authentication and access policy");
        }
        if output.len() > 1024 * 1024 {
            bail!("Bitwarden response exceeds size limit");
        }
        let document: serde_json::Value = serde_json::from_slice(&output)
            .map_err(|_| anyhow::anyhow!("Bitwarden returned invalid JSON"))?;
        if document["id"].as_str() != Some(id) {
            bail!("Bitwarden returned a different secret identifier");
        }
        let value = document["value"]
            .as_str()
            .context("Bitwarden response has no string value")?
            .to_owned();
        if let Some(persistent) = persistent {
            persistent.put(id, &value, refresh)?;
        }
        cache.insert(id.to_owned(), value.clone());
        Ok(value)
    }
}

impl Drop for Secrets {
    fn drop(&mut self) {
        // Opt-in diagnostics contain counts/timing only, never values or IDs.
        if std::env::var("TJIKUP_SECRET_TIMING").as_deref() == Ok("1") {
            eprintln!(
                "Bitwarden: {} requests, {:.3}s provider time; {} cache hits",
                self.requests.load(Ordering::Relaxed),
                self.request_millis.load(Ordering::Relaxed) as f64 / 1000.,
                self.cache_hits.load(Ordering::Relaxed)
            );
        }
    }
}

pub fn enable_refresh() -> Result<()> {
    std::env::set_var("TJIKUP_REFRESH_SECRETS", "all");
    std::env::set_var(
        "TJIKUP_SECRET_REFRESH_RUN",
        super::credential_cache::random_hex()?,
    );
    Ok(())
}

pub fn fish_quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}
