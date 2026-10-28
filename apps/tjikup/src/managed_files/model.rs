use super::secrets::{Bitwarden, Secrets};
use anyhow::{bail, Context, Result};
use minijinja::{Environment, UndefinedBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    version: u32,
    pub id: String,
    pub host: Option<String>,
    #[serde(default)]
    context: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    context_files: Vec<String>,
    #[serde(default)]
    entries_files: Vec<String>,
    #[serde(default)]
    partials: BTreeMap<String, String>,
    #[serde(default)]
    entries: Vec<Entry>,
    bitwarden: Option<Bitwarden>,
}

#[derive(Deserialize)]
struct Entry {
    path: String,
    #[serde(default)]
    group: String,
    #[serde(flatten)]
    content: Content,
    mode: Option<String>,
    uid: Option<u32>,
    gid: Option<u32>,
    when: Option<String>,
    #[serde(default)]
    create_only: bool,
    #[serde(default)]
    sensitive: bool,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Content {
    File {
        source: String,
        #[serde(default)]
        template: bool,
    },
    Symlink {
        target: String,
    },
    Directory,
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Snapshot {
    Absent,
    Directory {
        mode: u32,
        uid: u32,
        gid: u32,
    },
    File {
        sha256: String,
        mode: u32,
        uid: u32,
        gid: u32,
    },
    Symlink {
        target: String,
        uid: u32,
        gid: u32,
    },
}

pub struct Desired {
    pub path: PathBuf,
    pub snapshot: Snapshot,
    pub bytes: Vec<u8>,
    pub create_only: bool,
    pub sensitive: bool,
}

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn absolute(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path == Path::new("/")
        || path
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
    {
        bail!(
            "expected an absolute normalized file path: {}",
            path.display()
        );
    }
    if path.to_str().is_none() || path.as_os_str().as_encoded_bytes().contains(&0) {
        bail!("invalid file path");
    }
    Ok(())
}

fn source(base: &Path, name: &str) -> Result<Vec<u8>> {
    let relative = Path::new(name);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("source must be a relative path without traversal: {name}");
    }
    let path = base
        .join(relative)
        .canonicalize()
        .with_context(|| format!("source {name}"))?;
    if !path.starts_with(base) {
        bail!("source escapes manifest directory: {name}");
    }
    std::fs::read(path).with_context(|| format!("read source {name}"))
}

impl Manifest {
    pub fn load(
        path: &Path,
        context_file: Option<&Path>,
        group: Option<&str>,
    ) -> Result<(Self, Vec<Desired>)> {
        let path = path.canonicalize()?;
        let mut manifest: Self =
            serde_json::from_slice(&std::fs::read(&path)?).context("invalid file manifest")?;
        if manifest.version != 1 {
            bail!("expected file manifest version 1");
        }
        if let Some(host) = &manifest.host {
            if *host != super::hostname()? {
                bail!("manifest is for host {host}");
            }
        }
        if manifest.id.is_empty()
            || !manifest
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        {
            bail!("invalid manifest id");
        }
        let base = path.parent().unwrap();
        let mut context = BTreeMap::new();
        for file in &manifest.context_files {
            let values: BTreeMap<String, serde_json::Value> =
                serde_json::from_slice(&source(base, file)?).context("invalid context file")?;
            context.extend(values);
        }
        context.extend(manifest.context.clone());
        if let Some(file) = context_file {
            let values: BTreeMap<String, serde_json::Value> =
                serde_json::from_slice(&std::fs::read(file)?)
                    .context("invalid context override")?;
            context.extend(values);
        }
        for file in &manifest.entries_files {
            let entries: Vec<Entry> =
                serde_json::from_slice(&source(base, file)?).context("invalid entries file")?;
            manifest.entries.extend(entries);
        }
        if let Some(group) = group {
            if group.is_empty() || !manifest.entries.iter().any(|entry| entry.group == group) {
                bail!("unknown file group: {group}");
            }
            manifest.entries.retain(|entry| entry.group == group);
        }
        let mut env = Environment::new();
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        env.set_keep_trailing_newline(true);
        for (name, file) in &manifest.partials {
            env.add_template_owned(name.clone(), String::from_utf8(source(base, file)?)?)?;
        }
        env.add_filter("json_quote", |value: String| {
            serde_json::to_string(&value).unwrap()
        });
        env.add_filter("compose_quote", |value: String| {
            serde_json::to_string(&value.replace('$', "$$")).unwrap()
        });
        env.add_filter("fish_quote", |value: String| {
            super::secrets::fish_quote(&value)
        });
        let context = minijinja::Value::from_serialize(&context);
        let secrets = manifest
            .bitwarden
            .clone()
            .map(|mut config| {
                if let Some(path) = &config.access_token_file {
                    config.access_token_file = Some(env.render_str(path, &context)?);
                }
                Ok::<_, anyhow::Error>(std::sync::Arc::new(Secrets::new(config)?))
            })
            .transpose()?;
        let mut content_env = env.clone();
        if let Some(client) = &secrets {
            let client = client.clone();
            content_env.add_function("secret", move |id: String| {
                client.get(&id).map_err(|error| {
                    minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, error.to_string())
                })
            });
        }
        let mut paths = BTreeSet::new();
        let mut result = Vec::new();
        for entry in &manifest.entries {
            if let Some(condition) = &entry.when {
                let value = env.compile_expression(condition)?.eval(&context)?;
                if value.kind() != minijinja::value::ValueKind::Bool {
                    bail!("when must evaluate to a boolean");
                }
                if !value.is_true() {
                    continue;
                }
            }
            let path = PathBuf::from(
                env.render_str(&entry.path, &context)
                    .context("render destination")?,
            );
            absolute(&path)?;
            if !paths.insert(path.clone()) {
                bail!("duplicate destination: {}", path.display());
            }
            let uid = entry.uid.unwrap_or_else(|| unsafe { libc::geteuid() });
            let gid = entry.gid.unwrap_or_else(|| unsafe { libc::getegid() });
            if let Some(client) = &secrets {
                client
                    .used
                    .store(false, std::sync::atomic::Ordering::SeqCst);
            }
            let (snapshot, bytes) = match &entry.content {
                Content::File {
                    source: name,
                    template,
                } => {
                    let mut bytes = source(base, name)?;
                    if *template {
                        // Do not include template errors: they may quote secret context or source.
                        bytes = content_env.render_str(&String::from_utf8(bytes)?, &context)
                            .map_err(|_| anyhow::anyhow!("template rendering failed for {}; check syntax and required variables", path.display()))?.into_bytes();
                    }
                    if secrets
                        .as_ref()
                        .is_some_and(|client| client.used.load(std::sync::atomic::Ordering::SeqCst))
                        && !entry.sensitive
                    {
                        bail!("Bitwarden-backed files must declare sensitive: true");
                    }
                    let mode = u32::from_str_radix(entry.mode.as_deref().unwrap_or("0644"), 8)
                        .context("mode must be octal")?;
                    if mode > 0o777 {
                        bail!("special permission bits are unsupported");
                    }
                    if entry.sensitive && mode & 0o077 != 0 {
                        bail!("sensitive files require owner-only permissions");
                    }
                    (
                        Snapshot::File {
                            sha256: hash(&bytes),
                            mode,
                            uid,
                            gid,
                        },
                        bytes,
                    )
                }
                Content::Symlink { target } => {
                    if entry.mode.is_some() {
                        bail!("symlinks cannot declare a mode");
                    }
                    let target = env.render_str(target, &context).context("render symlink")?;
                    if target.is_empty() || target.contains('\0') {
                        bail!("invalid symlink target");
                    }
                    (Snapshot::Symlink { target, uid, gid }, Vec::new())
                }
                Content::Directory => {
                    let mode = u32::from_str_radix(entry.mode.as_deref().unwrap_or("0755"), 8)
                        .context("mode must be octal")?;
                    if mode > 0o777 || entry.sensitive && mode & 0o077 != 0 {
                        bail!("invalid directory permissions");
                    }
                    (Snapshot::Directory { mode, uid, gid }, Vec::new())
                }
                Content::Absent => {
                    if entry.mode.is_some()
                        || entry.uid.is_some()
                        || entry.gid.is_some()
                        || entry.create_only
                    {
                        bail!("absent entries cannot declare metadata or create_only");
                    }
                    (Snapshot::Absent, Vec::new())
                }
            };
            result.push(Desired {
                path,
                snapshot,
                bytes,
                create_only: entry.create_only,
                sensitive: entry.sensitive,
            });
        }
        for path in &paths {
            if path.ancestors().skip(1).any(|ancestor| {
                result.iter().any(|e| {
                    e.path == ancestor && !matches!(e.snapshot, Snapshot::Directory { .. })
                })
            }) {
                bail!("destinations overlap: {}", path.display());
            }
        }
        // Parents must be present before children, including explicitly private directories.
        result.sort_by_key(|e| (e.path.components().count(), e.path.clone()));
        Ok((manifest, result))
    }
}
