//! Local device identity and explicit subscriptions; profiles contain no secrets.
use crate::config::{Config, Recipe};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub version: u32,
    pub hostname: String,
    pub role: String,
    pub subscriptions: BTreeSet<String>,
}
pub fn hostname() -> Result<String> {
    let mut bytes = [0u8; 256];
    if unsafe { libc::gethostname(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(std::str::from_utf8(bytes.split(|b| *b == 0).next().unwrap())?.to_owned())
}
pub fn profile_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
        .join("tjikup/device.json")
}
impl Device {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 || !["desktop", "laptop", "server"].contains(&self.role.as_str()) {
            bail!("device profile requires version 1 and role desktop, laptop or server");
        }
        if self.hostname != hostname()? {
            bail!(
                "device profile belongs to {}; enroll this hostname with tjikup new device",
                self.hostname
            );
        }
        Ok(())
    }
    pub fn allows(&self, recipe: &Recipe) -> bool {
        (recipe.roles.is_empty() || recipe.roles.contains(&self.role))
            && (recipe.hosts.is_empty() || recipe.hosts.contains(&self.hostname))
    }
    pub fn select(&self, config: &mut Config) -> Result<()> {
        self.validate()?;
        let mut selected = BTreeSet::new();
        for original in &self.subscriptions {
            let id = config
                .subscription_aliases
                .get(original)
                .unwrap_or(original);
            let recipe = config
                .recipes
                .iter()
                .find(|r| &r.id == id)
                .with_context(|| format!("unknown recipe subscription {id}"))?;
            if !recipe.optional || !self.allows(recipe) {
                bail!("subscription {id} is not available for this device");
            }
            selected.insert(id.clone());
        }
        selected.extend(
            config
                .recipes
                .iter()
                .filter(|r| !r.optional && r.part_of.is_none() && self.allows(r))
                .map(|r| r.id.clone()),
        );
        loop {
            let before = selected.len();
            for recipe in &config.recipes {
                if recipe
                    .part_of
                    .as_ref()
                    .is_some_and(|id| selected.contains(id))
                    && self.allows(recipe)
                {
                    selected.insert(recipe.id.clone());
                }
            }
            for recipe in &config.recipes {
                if !selected.contains(&recipe.id) {
                    continue;
                }
                for id in &recipe.needs {
                    let dependency = config
                        .recipes
                        .iter()
                        .find(|r| &r.id == id)
                        .with_context(|| format!("missing dependency {id}"))?;
                    if !self.allows(dependency) {
                        bail!(
                            "{} requires {id}, which is unavailable for role {} on {}",
                            recipe.id,
                            self.role,
                            self.hostname
                        );
                    }
                    selected.insert(id.clone());
                }
            }
            if before == selected.len() {
                break;
            }
        }
        let subscriptions = selected.iter().cloned().collect::<Vec<_>>().join(",");
        config.excluded = config
            .recipes
            .iter()
            .filter(|r| !selected.contains(&r.id))
            .cloned()
            .collect();
        config.recipes.retain(|r| selected.contains(&r.id));
        for recipe in &mut config.recipes {
            recipe.after.retain(|id| selected.contains(id));
            recipe.env.insert("TJIKUP_ROLE".into(), self.role.clone());
            recipe
                .env
                .insert("TJIKUP_SUBSCRIPTIONS".into(), subscriptions.clone());
            // Ordering edges are resolved after operation selection, so a
            // file-only invocation never pulls installation or updates in.
        }
        Ok(())
    }
}
fn prompt(question: &str) -> Result<String> {
    print!("{question}");
    io::stdout().flush()?;
    let mut value = String::new();
    if io::stdin().read_line(&mut value)? == 0 {
        bail!("device enrollment cancelled: input closed");
    }
    Ok(value.trim().to_owned())
}
pub fn cli(args: Vec<String>, default_config: PathBuf) -> Result<i32> {
    let mut args = args.into_iter();
    if args.next().as_deref() != Some("device") {
        bail!("usage: tjikup new device [--config PATH] [--role ROLE --subscribe ID,ID] [--output PATH] [--replace]");
    }
    let mut path = default_config;
    let mut output = None;
    let mut role = None;
    let mut subscriptions = None;
    let mut replace = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => path = args.next().context("--config needs a path")?.into(),
            "--output" => {
                output = Some(PathBuf::from(args.next().context("--output needs a path")?))
            }
            "--role" => role = Some(args.next().context("--role needs a value")?),
            "--subscribe" => {
                subscriptions = Some(
                    args.next()
                        .context("--subscribe needs comma-separated IDs (or none)")?,
                )
            }
            "--replace" => replace = true,
            _ => bail!("unknown enrollment option {arg}"),
        }
    }
    let mut config = Config::catalog(&path, &BTreeSet::new())?;
    let output = output
        .or_else(|| config.device_file.clone())
        .unwrap_or_else(profile_path);
    if output.exists() && !replace {
        bail!(
            "{} already exists; use --replace to revise its subscriptions",
            output.display()
        );
    }
    let interactive = role.is_none() || subscriptions.is_none();
    if interactive && !io::stdin().is_terminal() {
        bail!("interactive enrollment needs a terminal; use --role and --subscribe for automation");
    }
    let role = match role {
        Some(role) => role,
        None => prompt("Device type [desktop/laptop/server]: ")?,
    };
    let mut device = Device {
        version: 1,
        hostname: hostname()?,
        role,
        subscriptions: BTreeSet::new(),
    };
    device.validate()?;
    let available: Vec<_> = config
        .recipes
        .iter()
        .filter(|r| r.optional && device.allows(r))
        .collect();
    let choice = match subscriptions {
        Some(value) => value,
        None => {
            println!("\nOptional recipes (dependencies are included automatically):");
            for (i, recipe) in available.iter().enumerate() {
                println!("  {}. {} [{}]", i + 1, recipe.label, recipe.id);
            }
            prompt("Select numbers or recipe IDs, separated by commas; all or none [none]: ")?
        }
    };
    if choice == "all" {
        device
            .subscriptions
            .extend(available.iter().map(|r| r.id.clone()));
    } else if !choice.is_empty() && choice != "none" {
        for value in choice.split(',').map(str::trim) {
            let id = match value.parse::<usize>() {
                Ok(n) => available
                    .get(n.wrapping_sub(1))
                    .with_context(|| format!("invalid recipe number {n}"))?
                    .id
                    .clone(),
                Err(_) => value.to_owned(),
            };
            device.subscriptions.insert(id);
        }
    }
    device.select(&mut config)?;
    config.validate()?;
    println!(
        "\n{}: {}\nProfile: {}\nResolved recipes:",
        device.hostname,
        device.role,
        output.display()
    );
    for recipe in &config.recipes {
        println!("  {}", recipe.id);
    }
    if interactive && prompt("Save this device profile? [y/N]: ")?.to_lowercase() != "y" {
        bail!("device enrollment cancelled");
    }
    let parent = output
        .parent()
        .context("profile needs a parent directory")?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(file.as_file_mut(), &device)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    if replace {
        file.persist(&output)?;
    } else {
        file.persist_noclobber(&output)?;
    }
    println!("Saved. Review with tjikup --plan before running recipes.");
    Ok(0)
}
pub fn load(path: &Path) -> Result<Option<(Device, Vec<u8>)>> {
    match fs::read(path) {
        Ok(bytes) => {
            let device: Device =
                serde_json::from_slice(&bytes).context("invalid device profile")?;
            device.validate()?;
            Ok(Some((device, bytes)))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
