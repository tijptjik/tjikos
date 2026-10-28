//! Native maintenance-script rendering. Inputs are explicit; diagnostics never
//! include rendered contents, provider responses, or context values.
use crate::managed_files::secrets::{fish_quote, Bitwarden, Secrets};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use minijinja::{Environment, UndefinedBehavior};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Component, Path},
    sync::Arc,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    version: u32,
    #[serde(default)]
    context: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    context_files: Vec<String>,
    #[serde(default)]
    includes: BTreeMap<String, String>,
    bitwarden: Option<Bitwarden>,
}

fn read(base: &Path, name: &str) -> Result<Vec<u8>> {
    let path = Path::new(name);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("template input must be relative without traversal");
    }
    let path = base
        .join(path)
        .canonicalize()
        .context("missing template input")?;
    if !path.starts_with(base) {
        bail!("template input escapes its directory");
    }
    std::fs::read(path).context("cannot read template input")
}

pub struct Renderer {
    environment: Environment<'static>,
    context: minijinja::Value,
}
impl Renderer {
    pub fn load(path: &Path, runtime: Option<&[u8]>) -> Result<Self> {
        let path = path.canonicalize()?;
        let base = path.parent().unwrap();
        let inputs: Inputs = serde_json::from_slice(&std::fs::read(&path)?)
            .map_err(|_| anyhow::anyhow!("invalid native template inputs"))?;
        if inputs.version != 1 {
            bail!("expected native template inputs version 1");
        }
        let mut context = BTreeMap::new();
        for file in inputs.context_files {
            let values: BTreeMap<String, serde_json::Value> =
                serde_json::from_slice(&read(base, &file)?)
                    .map_err(|_| anyhow::anyhow!("invalid native template context"))?;
            context.extend(values);
        }
        context.extend(inputs.context);
        if let Some(bytes) = runtime {
            let values: BTreeMap<String, serde_json::Value> = serde_json::from_slice(bytes)
                .map_err(|_| anyhow::anyhow!("native context command must return a JSON object"))?;
            context.extend(values);
        }
        let mut env = Environment::new();
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        env.set_keep_trailing_newline(true);
        let mut includes = BTreeMap::new();
        for (name, file) in inputs.includes {
            includes.insert(
                name,
                String::from_utf8(read(base, &file)?)
                    .map_err(|_| anyhow::anyhow!("native include is not UTF-8"))?,
            );
        }
        env.add_function(
            "include",
            move |name: String| -> std::result::Result<String, minijinja::Error> {
                includes.get(&name).cloned().ok_or_else(|| {
                    minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "undeclared native include",
                    )
                })
            },
        );
        env.add_filter("b64enc", |value: String| STANDARD.encode(value));
        env.add_filter("sha256", |value: String| {
            format!("{:x}", Sha256::digest(value.as_bytes()))
        });
        env.add_filter("json_quote", |value: String| {
            serde_json::to_string(&value).unwrap()
        });
        env.add_filter("fish_quote", |value: String| fish_quote(&value));
        let context = minijinja::Value::from_serialize(context);
        if let Some(mut config) = inputs.bitwarden {
            if let Some(path) = &config.access_token_file {
                config.access_token_file = Some(
                    env.render_str(path, &context)
                        .map_err(|_| anyhow::anyhow!("invalid Bitwarden token path template"))?,
                );
            }
            let secrets = Arc::new(Secrets::new(config)?);
            env.add_function("secret", move |id: String| {
                secrets.get(&id).map_err(|_| {
                    minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "Bitwarden lookup failed",
                    )
                })
            });
        }
        Ok(Self {
            environment: env,
            context,
        })
    }
    pub fn render(&self, path: &Path) -> Result<Vec<u8>> {
        let source = std::fs::read_to_string(path).context("cannot read native template")?;
        self.environment
            .render_str(&source, &self.context)
            .map(String::into_bytes)
            .map_err(|_| {
                anyhow::anyhow!(
                    "native template rendering failed; check syntax, context and secret access"
                )
            })
    }
}
