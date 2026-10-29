use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Presentation {
    pub url: String,
    pub links: Vec<PresentationLink>,
    pub splash: bool,
    pub grouped_results: bool,
    pub alphabetical_results: bool,
    pub icons: String,
    pub icon_aliases: BTreeMap<String, String>,
    pub glyphs: BTreeMap<String, String>,
    pub titles: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationLink {
    pub title: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Display {
    pub icon: String,
    pub section: String,
    pub verb: String,
    pub reports: bool,
    pub quiet: bool,
    pub repository: String,
    pub operation: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manager {
    pub command: Vec<String>,
    pub source: String,
    pub config: Option<String>,
    pub config_template: Option<String>,
    #[serde(default)]
    pub refresh_retry: bool,
    pub skip_conflicts_env: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub environment: String,
    pub equals: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Git {
    pub repository: String,
    pub operation: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Autocommit {
    pub repository: String,
    pub files: Vec<String>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub name: String,
    pub source: String,
    pub syntax: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Render {
    /// Opt in only for maintenance scripts containing no credentials.
    #[serde(default)]
    pub cache: bool,
    #[serde(default)]
    pub manager: String,
    pub native: Option<String>,
    pub context_command: Option<Vec<String>>,
    pub templates: Vec<Template>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialRecovery {
    pub secret_ids: Vec<String>,
    pub failure_codes: Vec<i32>,
    pub refresh: Vec<String>,
    pub retry_safe: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub operation: Option<crate::selection::Operation>,
    pub scope: Option<crate::selection::Scope>,
    #[serde(default)]
    pub application: String,
    #[serde(default)]
    pub ecosystem: String,
    pub part_of: Option<String>,
    #[serde(skip)]
    pub installed_gate: bool,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub after: Vec<String>,
    #[serde(default)]
    pub display: Display,
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub resources: Vec<String>,
    pub check: Option<Vec<String>>,
    pub when: Option<Condition>,
    pub apply: Option<Vec<String>>,
    pub preview: Option<Vec<String>>,
    pub credential_recovery: Option<CredentialRecovery>,
    pub autocommit: Option<Autocommit>,
    pub git: Option<Git>,
    pub files: Option<String>,
    pub render: Option<Render>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_cwd")]
    pub cwd: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub skip_codes: Vec<i32>,
    #[serde(default)]
    pub stop_codes: Vec<i32>,
    #[serde(skip)]
    pub prerequisite: bool,
}
fn default_cwd() -> String {
    "{config_dir}".into()
}
fn default_timeout() -> u64 {
    300
}
fn default_jobs() -> usize {
    4
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    #[serde(default)]
    device_file: Option<String>,
    #[serde(default)]
    subscription_aliases: BTreeMap<String, String>,
    #[serde(default)]
    legacy_hosts: Vec<String>,
    #[serde(default)]
    presentation: Presentation,
    version: u32,
    #[serde(default)]
    title: String,
    #[serde(default = "default_jobs")]
    jobs: usize,
    #[serde(default)]
    locks: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    flags: BTreeMap<String, String>,
    #[serde(default)]
    includes: Vec<String>,
    #[serde(default)]
    managers: BTreeMap<String, Manager>,
    #[serde(default)]
    prerequisites: Vec<Recipe>,
    #[serde(default)]
    recipes: Vec<Recipe>,
}

#[derive(Debug)]
pub struct Config {
    pub device_file: Option<PathBuf>,
    pub subscription_aliases: BTreeMap<String, String>,
    pub legacy_hosts: Vec<String>,
    pub excluded: Vec<Recipe>,
    pub show_other_devices: bool,
    pub show_skips: bool,
    pub presentation: Presentation,
    pub title: String,
    pub jobs: usize,
    pub locks: Vec<String>,
    pub managers: BTreeMap<String, Manager>,
    pub recipes: Vec<Recipe>,
    pub snapshots: BTreeMap<PathBuf, Vec<u8>>,
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
}
pub fn argv(value: &[String]) -> Result<()> {
    if value.is_empty() || value[0].is_empty() || value.iter().any(|s| s.contains('\0')) {
        bail!("expected a nonempty argv array without NUL");
    }
    Ok(())
}

/// Expand only explicit config substitutions. Shell syntax is never evaluated.
pub fn expand(text: &str, directory: &Path, env: &BTreeMap<String, String>) -> Result<String> {
    let text = text.replace("{config_dir}", &directory.to_string_lossy());
    let mut output = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            output.push(c);
            continue;
        }
        if chars.peek() == Some(&'$') {
            chars.next();
            output.push('$');
            continue;
        }
        let braced = chars.peek() == Some(&'{');
        if braced {
            chars.next();
        }
        let mut name = String::new();
        while chars
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
        {
            name.push(chars.next().unwrap());
        }
        if name.is_empty() || (braced && chars.next() != Some('}')) {
            bail!("invalid variable substitution in {text:?}; use $$ for a literal dollar");
        }
        output.push_str(
            env.get(&name)
                .with_context(|| format!("undefined variable {name}"))?,
        );
    }
    if let Some(rest) = output.strip_prefix("~/") {
        output = format!("{}/{}", env.get("HOME").context("HOME is unset")?, rest);
    }
    if output.contains('\0') {
        bail!("NUL in configuration");
    }
    Ok(output)
}

fn environment(
    raw: &BTreeMap<String, String>,
    directory: &Path,
    base: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let mut env = base.clone();
    let mut pending = raw.clone();
    // A reference to another declared variable must not accidentally use an old inherited value.
    for key in raw.keys() {
        if key.is_empty()
            || !key
                .bytes()
                .enumerate()
                .all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || i > 0 && c.is_ascii_digit())
        {
            bail!("invalid environment variable {key}");
        }
        env.remove(key);
    }
    while !pending.is_empty() {
        let before = pending.len();
        for (key, value) in pending.clone() {
            if let Ok(value) = expand(&value, directory, &env) {
                env.insert(key.clone(), value);
                pending.remove(&key);
            }
        }
        if before == pending.len() {
            bail!(
                "unresolved or cyclic environment variables: {:?}",
                pending.keys().collect::<Vec<_>>()
            );
        }
    }
    Ok(env)
}

fn expand_value(
    value: &mut serde_json::Value,
    directory: &Path,
    env: &BTreeMap<String, String>,
) -> Result<()> {
    match value {
        serde_json::Value::String(text) => *text = expand(text, directory, env)?,
        serde_json::Value::Array(items) => {
            for item in items {
                expand_value(item, directory, env)?;
            }
        }
        serde_json::Value::Object(items) => {
            for (_, item) in items {
                expand_value(item, directory, env)?;
            }
        }
        _ => (),
    }
    Ok(())
}

impl Config {
    pub fn load(path: &Path, flags: &BTreeSet<String>) -> Result<Self> {
        Self::load_selected(path, flags, &crate::selection::Selection::default())
    }

    pub fn load_selected(
        path: &Path,
        flags: &BTreeSet<String>,
        selection: &crate::selection::Selection,
    ) -> Result<Self> {
        let mut config = Self::catalog(path, flags)?;
        if let Some(path) = &config.device_file.clone() {
            if let Some((device, bytes)) = crate::device::load(path)? {
                config.snapshots.insert(path.clone(), bytes);
                device.select(&mut config)?;
            } else if !config.legacy_hosts.contains(&crate::device::hostname()?) {
                bail!(
                    "device is not enrolled; run tjikup new device before applying this workflow"
                );
            }
        }
        selection.apply(&mut config)?;
        config.validate()?;
        Ok(config)
    }

    pub fn catalog(path: &Path, flags: &BTreeSet<String>) -> Result<Self> {
        let path = path
            .canonicalize()
            .with_context(|| format!("cannot open {}", path.display()))?;
        let mut config = Self {
            device_file: None,
            subscription_aliases: BTreeMap::new(),
            legacy_hosts: vec![],
            excluded: vec![],
            show_other_devices: false,
            show_skips: false,
            presentation: Presentation::default(),
            title: "Tjikup".into(),
            jobs: 4,
            locks: vec![],
            managers: BTreeMap::new(),
            recipes: vec![],
            snapshots: BTreeMap::new(),
        };
        let mut env: BTreeMap<String, String> = std::env::vars().collect();
        if !env.contains_key("XDG_CONFIG_HOME") {
            if let Some(home) = env.get("HOME") {
                env.insert("XDG_CONFIG_HOME".into(), format!("{home}/.config"));
            }
        }
        config.read(&path, &env, flags, true)?;
        let prerequisites: Vec<_> = config
            .recipes
            .iter()
            .filter(|r| r.prerequisite)
            .map(|r| r.id.clone())
            .collect();
        for recipe in &mut config.recipes {
            if !recipe.prerequisite {
                recipe.needs.extend(prerequisites.iter().cloned());
            }
            recipe.needs.sort();
            recipe.needs.dedup();
        }
        config.validate()?;
        for (old, target) in &config.subscription_aliases {
            if !identifier(old)
                || config.recipes.iter().any(|r| &r.id == old)
                || !config.recipes.iter().any(|r| &r.id == target && r.optional)
            {
                bail!("subscription alias {old} must name a retired ID and an existing optional recipe");
            }
        }
        // Without a profile, preserve the existing fleet's complete workflow.
        // Profile selection adds these edges only after subscriptions are known.
        Ok(config)
    }

    fn read(
        &mut self,
        path: &Path,
        inherited: &BTreeMap<String, String>,
        selected: &BTreeSet<String>,
        root: bool,
    ) -> Result<()> {
        let path = path.canonicalize()?;
        if self.snapshots.contains_key(&path) {
            bail!("duplicate or cyclic include: {}", path.display());
        }
        let bytes = std::fs::read(&path)?;
        let document: Document = serde_json::from_slice(&bytes)
            .with_context(|| format!("invalid configuration {}", path.display()))?;
        if document.version != 2 {
            bail!("{}: expected recipe schema version 2; migrate the version 1 fleet configuration first", path.display());
        }
        self.snapshots.insert(path.clone(), bytes);
        let directory = path.parent().unwrap();
        let mut env = environment(&document.env, directory, inherited)?;
        if root {
            self.legacy_hosts = document.legacy_hosts;
            self.subscription_aliases = document.subscription_aliases;
            self.device_file = document
                .device_file
                .as_ref()
                .map(|path| expand(path, directory, &env).map(PathBuf::from))
                .transpose()?;
            for flag in selected {
                if !document.flags.contains_key(flag) {
                    bail!("unknown workflow flag --{flag}");
                }
            }
            self.title = if document.title.is_empty() {
                "Tjikup".into()
            } else {
                document.title
            };
            self.jobs = document.jobs;
            let mut value = serde_json::to_value(document.presentation)?;
            expand_value(&mut value, directory, &env)?;
            self.presentation = serde_json::from_value(value)?;
        } else if !document.flags.is_empty()
            || document.device_file.is_some()
            || !document.legacy_hosts.is_empty()
            || !document.subscription_aliases.is_empty()
        {
            bail!("workflow flags and device_file belong in the root configuration");
        }
        for (name, variable) in document.flags {
            if !identifier(&name)
                || [
                    "help",
                    "check",
                    "dry-run",
                    "config",
                    "plan",
                    "jobs",
                    "version",
                    "refresh-secrets",
                    "alphabetical",
                    "show-skips",
                    "show-skips-and-other-devices",
                    "demo",
                    "apply-dotfiles",
                    "apply-sysfiles",
                    "apply-package-updates",
                    "skip-package-updates",
                ]
                .contains(&name.as_str())
            {
                bail!("invalid workflow flag {name}");
            }
            environment(
                &BTreeMap::from([(variable.clone(), "0".into())]),
                directory,
                &env,
            )?;
            env.insert(
                variable,
                if selected.contains(&name) { "1" } else { "0" }.into(),
            );
        }
        for lock in document.locks {
            self.locks.push(expand(&lock, directory, &env)?);
        }
        for (name, manager) in document.managers {
            let mut value = serde_json::to_value(manager)?;
            expand_value(&mut value, directory, &env)?;
            if self
                .managers
                .insert(name.clone(), serde_json::from_value(value)?)
                .is_some()
            {
                bail!("duplicate manager {name}");
            }
        }
        for (mut recipe, prerequisite) in document
            .prerequisites
            .into_iter()
            .map(|r| (r, true))
            .chain(document.recipes.into_iter().map(|r| (r, false)))
        {
            let recipe_env = environment(&recipe.env, directory, &env)?;
            recipe.env.clear();
            let mut value = serde_json::to_value(recipe)?;
            expand_value(&mut value, directory, &recipe_env)?;
            recipe = serde_json::from_value(value)?;
            recipe.env = recipe_env;
            recipe.prerequisite = prerequisite;
            recipe.cwd = directory.join(&recipe.cwd).to_string_lossy().into_owned();
            if recipe.label.is_empty() {
                recipe.label = if prerequisite {
                    recipe
                        .id
                        .strip_prefix("require:")
                        .unwrap_or(&recipe.id)
                        .into()
                } else {
                    recipe.id.clone()
                };
            }
            self.recipes.push(recipe);
        }
        for include in document.includes {
            let include = directory.join(expand(&include, directory, &env)?);
            self.read(&include, &env, selected, false)?;
        }
        Ok(())
    }

    pub fn unchanged(&self) -> Result<()> {
        for (path, before) in &self.snapshots {
            if std::fs::read(path).as_ref().ok() != Some(before) {
                bail!("recipe configuration changed during this run: {}; rerun tjikup to load the new graph", path.display());
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if self.jobs == 0 || self.jobs > 256 {
            bail!("jobs must be between 1 and 256");
        }
        if self.recipes.is_empty() {
            bail!("no recipes configured");
        }
        let mut ids = BTreeSet::new();
        let mut rendered = BTreeSet::new();
        let mut display_cells = BTreeSet::new();
        for (name, manager) in &self.managers {
            if !identifier(name) || manager.source.is_empty() {
                bail!("invalid manager {name}");
            }
            argv(&manager.command)?;
        }
        for r in &self.recipes {
            if r.operation == Some(crate::selection::Operation::Configure) && r.scope.is_none() {
                bail!("{}: configure recipes require a home or system scope", r.id);
            }
            if r.roles
                .iter()
                .any(|role| !["desktop", "laptop", "server"].contains(&role.as_str()))
            {
                bail!("{}: unknown device role", r.id);
            }
            if let Some(condition) = &r.when {
                if !r.env.contains_key(&condition.environment) {
                    bail!(
                        "{}: unknown condition variable {}",
                        r.id,
                        condition.environment
                    );
                }
            }
            if !identifier(&r.id) || !ids.insert(r.id.clone()) {
                bail!("invalid or duplicate recipe id {}", r.id);
            }
            if r.display.repository.is_empty() != r.display.operation.is_empty() {
                bail!(
                    "{}: repository display requires both repository and operation",
                    r.id
                );
            }
            if !r.display.repository.is_empty()
                && !display_cells.insert((&r.display.repository, &r.display.operation))
            {
                bail!("{}: duplicate repository display operation", r.id);
            }
            if !r.display.repository.is_empty()
                && !["pull", "push"].contains(&r.display.operation.as_str())
            {
                bail!(
                    "{}: repository display requires pull or push operation",
                    r.id
                );
            }
            if r.timeout == 0 {
                bail!("{}: timeout must be positive", r.id);
            }
            for resource in &r.resources {
                if !identifier(resource) {
                    bail!("{}: invalid resource {resource}", r.id);
                }
            }
            let actions = usize::from(r.apply.is_some())
                + usize::from(r.files.is_some())
                + usize::from(r.render.is_some())
                + usize::from(r.autocommit.is_some())
                + usize::from(r.git.is_some());
            if actions > 1 || actions == 0 && r.check.is_none() {
                bail!(
                    "{}: provide one apply action, or a check-only prerequisite",
                    r.id
                );
            }
            if r.prerequisite && (r.check.is_none() || actions != 0) {
                bail!("{}: prerequisites require only a check command", r.id);
            }
            for cmd in [&r.check, &r.apply, &r.preview].into_iter().flatten() {
                argv(cmd).with_context(|| r.id.clone())?;
            }
            let mut codes = BTreeSet::new();
            for code in r.skip_codes.iter().chain(&r.stop_codes) {
                if !(1..=255).contains(code) || !codes.insert(code) {
                    bail!("{}: invalid or overlapping exit codes", r.id);
                }
            }
            if let Some(recovery) = &r.credential_recovery {
                if !recovery.retry_safe
                    || recovery.secret_ids.is_empty()
                    || recovery.failure_codes.is_empty()
                    || r.files.is_some()
                    || r.render.is_some()
                    || r.git.is_some()
                    || r.autocommit.is_some()
                {
                    bail!("{}: credential recovery requires declared secrets and retry-safe check/apply commands", r.id);
                }
                argv(&recovery.refresh)?;
                for id in &recovery.secret_ids {
                    if id.len() != 36
                        || !id.bytes().enumerate().all(|(i, c)| {
                            if [8, 13, 18, 23].contains(&i) {
                                c == b'-'
                            } else {
                                c.is_ascii_hexdigit()
                            }
                        })
                    {
                        bail!("{}: invalid credential secret ID", r.id);
                    }
                }
                for code in &recovery.failure_codes {
                    if !(2..=125).contains(code) || !codes.insert(code) {
                        bail!("{}: credential failures need dedicated, non-overlapping exit codes (not generic exit 1)", r.id);
                    }
                }
            }
            if let Some(name) = &r.files {
                if !self.managers.contains_key(name) {
                    bail!("unknown manager {name}");
                }
            }
            if let Some(git) = &r.git {
                if !["pull-rebase", "pull-ff", "push"].contains(&git.operation.as_str()) {
                    bail!("{}: invalid Git operation", r.id);
                }
            }
            if let Some(batch) = &r.render {
                if batch.templates.is_empty()
                    || (batch.native.is_some() && !batch.manager.is_empty())
                    || (batch.native.is_none() && !self.managers.contains_key(&batch.manager))
                    || (batch.native.is_none() && (batch.context_command.is_some() || batch.cache))
                {
                    bail!("{}: invalid render batch", r.id);
                }
                if let Some(command) = &batch.context_command {
                    argv(command)?;
                }
                for template in &batch.templates {
                    if !identifier(&template.name) || !rendered.insert(&template.name) {
                        bail!(
                            "invalid or duplicate rendered script name {}",
                            template.name
                        );
                    }
                    argv(&template.syntax)?;
                    if !template.syntax.iter().any(|s| s.contains("{script}")) {
                        bail!(
                            "{}: syntax command must reference {{script}}",
                            template.name
                        );
                    }
                }
            }
            if let Some(commit) = &r.autocommit {
                if commit.files.is_empty() || commit.message.is_empty() {
                    bail!("{}: autocommit needs exact files and a message", r.id);
                }
                for file in &commit.files {
                    if file.is_empty()
                        || Path::new(file)
                            .components()
                            .any(|c| !matches!(c, std::path::Component::Normal(_)))
                        || file.split('/').any(|s| s == ".git")
                    {
                        bail!(
                            "{}: autocommit requires literal repository-relative files",
                            r.id
                        );
                    }
                }
            }
        }
        for recipe in &self.recipes {
            let mut ancestors = BTreeSet::from([recipe.id.as_str()]);
            let mut parent = recipe.part_of.as_deref();
            while let Some(id) = parent {
                if !ancestors.insert(id) {
                    bail!("{}: cycle in part_of subscriptions", recipe.id);
                }
                let owner = self
                    .recipes
                    .iter()
                    .find(|r| r.id == id)
                    .with_context(|| format!("{}: unknown part_of recipe {id}", recipe.id))?;
                parent = owner.part_of.as_deref();
            }
        }
        let mut done = BTreeSet::new();
        while done.len() < ids.len() {
            let before = done.len();
            for r in &self.recipes {
                if r.needs
                    .iter()
                    .chain(&r.after)
                    .chain(r.part_of.iter())
                    .any(|n| !ids.contains(n))
                {
                    bail!("{}: unknown dependency", r.id);
                }
                if r.needs.iter().chain(&r.after).all(|n| done.contains(n)) {
                    done.insert(r.id.clone());
                }
            }
            if before == done.len() {
                bail!("dependency cycle in recipe graph");
            }
        }
        Ok(())
    }
}
