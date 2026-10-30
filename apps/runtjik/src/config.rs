use crate::util::*;
use anyhow::{bail, Result};
use serde_json::Value;
use std::{fs, path::PathBuf};
pub fn contract() -> &'static Value {
    static CONTRACT: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    CONTRACT.get_or_init(|| serde_json::from_str(include_str!("contract.json")).unwrap())
}
pub fn path() -> PathBuf {
    std::env::var_os("RUNTJIK_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg("config").join("runtjik.conf"))
}
fn same_type(a: &Value, b: &Value) -> bool {
    matches!(
        (a, b),
        (Value::Bool(_), Value::Bool(_))
            | (Value::String(_), Value::String(_))
            | (Value::Array(_), Value::Array(_))
            | (Value::Object(_), Value::Object(_))
    ) || (a.is_i64() && b.is_i64())
        || (a.is_f64() && b.is_number())
}
pub fn load() -> Result<Value> {
    let mut settings = contract()["DEFAULTS"].clone();
    let source = match fs::read_to_string(path()) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let raw: Value = serde_json::to_value(toml::from_str::<toml::Table>(&source)?)?;
    for (section, values) in raw.as_object().unwrap() {
        let defaults = settings
            .get_mut(section)
            .ok_or_else(|| anyhow::anyhow!("Unknown section: {section}"))?;
        let fields = values
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Expected table: {section}"))?;
        for (key, v) in fields {
            if section == "plugin_settings" {
                if !identifier(key) || !v.is_object() {
                    bail!("Invalid plugin settings");
                }
                let table = defaults
                    .as_object_mut()
                    .unwrap()
                    .entry(key)
                    .or_insert(serde_json::json!({}));
                for (k, v) in v.as_object().unwrap() {
                    table[k] = v.clone();
                }
            } else {
                if section != "overrides" {
                    let old = defaults
                        .get(key)
                        .ok_or_else(|| anyhow::anyhow!("Unknown setting: {section}.{key}"))?;
                    if !same_type(old, v) {
                        bail!("Invalid type: {section}.{key}");
                    }
                }
                defaults[key] = v.clone();
            }
        }
    }
    for (key, lo, hi) in [
        ("columns", 2, 10),
        ("width", 420, 1800),
        ("height", 320, 1400),
        ("icon_size", 24, 128),
        ("font_size", 10, 24),
        ("max_results", 10, 500),
    ] {
        range(&settings["ui"][key], lo, hi, key)?;
    }
    if !["grid", "list"].contains(&s(&settings["ui"]["view"])) {
        bail!("Invalid ui.view");
    }
    range(
        &settings["index"]["refresh_seconds"],
        1,
        3600,
        "refresh_seconds",
    )?;
    argv(&settings["launch"]["terminal"])?;
    for (_, v) in settings["overrides"].as_object().unwrap() {
        let fields = v
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Invalid override"))?;
        for (k, v) in fields {
            if !["argv", "window_classes"].contains(&k.as_str()) {
                bail!("Unknown override");
            }
            argv(v)?;
        }
    }
    for name in settings["plugins"]["enabled"].as_array().unwrap() {
        if !identifier(s(name)) {
            bail!("Invalid provider name");
        }
    }
    let defaults = contract();
    for name in ["Words", "Clipboard", "Screenshots", "Recorder", "Browser"] {
        let values = &settings["plugin_settings"][name];
        for (k, v) in values.as_object().unwrap() {
            let old = &defaults["DEFAULTS"]["plugin_settings"][name][k];
            if old.is_null() || !same_type(old, v) {
                bail!("Invalid setting: {name}.{k}");
            }
        }
        if name == "Words" {
            for (key, allowed) in [
                (
                    "define_sources",
                    vec!["free_dictionary", "wiktionary", "urban_dictionary"],
                ),
                ("synonym_sources", vec!["datamuse", "free_dictionary"]),
            ] {
                let mut seen = std::collections::HashSet::new();
                for v in values[key].as_array().unwrap() {
                    if !allowed.contains(&s(v)) || !seen.insert(s(v)) {
                        bail!("Invalid Words source");
                    }
                }
            }
            range(
                &values["max_results_per_source"],
                1,
                50,
                "max_results_per_source",
            )?;
            range(&values["timeout_seconds"], 1, 15, "timeout_seconds")?;
        } else if name == "Browser" {
            range(&values["columns"], 1, 6, "Browser.columns")?;
        } else {
            range(&values["max_results"], 1, 200, "max_results")?;
        }
        if name == "Screenshots" || name == "Recorder" {
            for key in ["gallery_columns", "gallery_rows"] {
                range(&values[key], 2, 6, key)?;
            }
            capture_directory(s(&values["directory"]))?;
        }
        if name == "Screenshots" {
            argv(&values["viewer"])?;
        }
        if name == "Recorder" {
            range(&values["fps"], 1, 120, "fps")?;
            range(&values["countdown_seconds"], 0, 10, "countdown_seconds")?;
        }
    }
    Ok(settings)
}
fn range(v: &Value, lo: i64, hi: i64, key: &str) -> Result<()> {
    if !v.as_i64().map(|n| (lo..=hi).contains(&n)).unwrap_or(false) {
        bail!("{key} must be an integer between {lo} and {hi}");
    }
    Ok(())
}
fn argv(v: &Value) -> Result<()> {
    if !v
        .as_array()
        .map(|a| {
            !a.is_empty()
                && a.iter()
                    .all(|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(false))
        })
        .unwrap_or(false)
    {
        bail!("Expected nonempty string array");
    }
    Ok(())
}
pub fn identifier(s: &str) -> bool {
    let mut c = s.chars();
    c.next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
        && c.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn resolved(path: &std::path::Path) -> PathBuf {
    if let Ok(p) = path.canonicalize() {
        return p;
    }
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::CurDir => {}
            other => {
                result.push(other);
                if let Ok(p) = result.canonicalize() {
                    result = p;
                }
            }
        }
    }
    result
}
fn capture_directory(value: &str) -> Result<()> {
    let path = if let Some(rest) = value.strip_prefix("xdg:") {
        let (kind, suffix) = rest.split_once('/').unwrap_or((rest, ""));
        if !["PICTURES", "VIDEOS"].contains(&kind)
            || std::path::Path::new(suffix)
                .components()
                .any(|c| c == std::path::Component::ParentDir)
        {
            bail!("Invalid XDG capture directory");
        }
        let mut base = home().join(if kind == "PICTURES" {
            "Pictures"
        } else {
            "Videos"
        });
        let user_dirs = xdg("config").parent().unwrap().join("user-dirs.dirs");
        if let Ok(text) = fs::read_to_string(user_dirs) {
            for line in text.lines() {
                if let Some(raw) = line.strip_prefix(&format!("XDG_{kind}_DIR=")) {
                    if let Some(words) = shlex::split(raw) {
                        if words.len() == 1 {
                            base =
                                PathBuf::from(words[0].replace("$HOME", &home().to_string_lossy()));
                        }
                    }
                }
            }
        }
        base.join(suffix)
    } else if let Some(rest) = value.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(value)
    };
    if !path.is_absolute() {
        bail!("Capture directory must be absolute or start with ~/");
    }
    let path = resolved(&path);
    let root = resolved(&crate::compat::root());
    if path.starts_with(&root)
        || root
            .ancestors()
            .find(|p| p.join(".git").exists())
            .is_some_and(|p| p != home() && path.starts_with(p))
    {
        bail!("Captures must be stored outside the repository/application directory");
    }
    Ok(())
}
