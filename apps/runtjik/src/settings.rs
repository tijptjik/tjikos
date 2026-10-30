//! Comment-preserving TOML edits with a lock, optimistic editor check and atomic publication.
use crate::{config, util::*};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::{
        fd::AsRawFd,
        unix::fs::{OpenOptionsExt, PermissionsExt},
    },
    path::Path,
};
pub fn featured(settings: &Value) -> Vec<(String, String)> {
    let contract = config::contract();
    let mut rows: Vec<_> = contract["featured"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (s(&r[0]).to_owned(), s(&r[1]).to_owned()))
        .collect();
    let mut names = strings(&contract["plugin_order"]);
    for name in settings["plugin_settings"].as_object().unwrap().keys() {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    for name in names {
        let Some(options) = settings["plugin_settings"][&name].as_object() else {
            continue;
        };
        let mut keys = strings(&contract["option_order"][&name]);
        for k in options.keys() {
            if !keys.contains(k) {
                keys.push(k.clone());
            }
        }
        for k in keys {
            let v = &options[&k];
            if config::identifier(&k) && !v.is_object() && !v.is_null() {
                let key = format!("plugin_settings.{name}.{k}");
                let description = match key.as_str() {
                    "plugin_settings.Words.define_sources" => {
                        "Sources: free_dictionary, wiktionary, urban_dictionary (optional slang)"
                            .into()
                    }
                    "plugin_settings.Words.synonym_sources" => {
                        "Sources: datamuse, free_dictionary".into()
                    }
                    _ => format!("{name} plugin · {}", k.replace('_', " ")),
                };
                rows.push((key, description));
            }
        }
    }
    rows
}
pub fn get<'a>(settings: &'a Value, key: &str) -> &'a Value {
    key.split('.').fold(settings, |v, k| &v[k])
}
pub fn display(value: &Value) -> String {
    match value {
        Value::Array(a) => format!("[{}]", a.iter().map(display).collect::<Vec<_>>().join(", ")),
        _ => value.to_string(),
    }
}
pub fn value(key: &str, text: &str) -> Result<Value> {
    if !featured(&config::load()?).iter().any(|(k, _)| k == key) {
        bail!("This setting is not exposed by /config; edit runtjik.conf directly");
    }
    match toml::from_str::<toml::Table>(&format!("value = {text}")) {
        Ok(t) => Ok(serde_json::to_value(&t["value"])?),
        Err(_) if key == "ui.view" && ["grid", "list"].contains(&text) => Ok(json!(text)),
        Err(_) => bail!("Use a TOML value, for example 6, true, or \"list\""),
    }
}
pub fn edit(source: &str, key: &str, value: &Value) -> Result<String> {
    let mut doc = source.parse::<toml_edit::DocumentMut>()?;
    let parsed = format!("value = {}", display(value)).parse::<toml_edit::DocumentMut>()?;
    let mut item = parsed["value"].clone();
    let parts: Vec<_> = key.split('.').collect();
    let mut target = doc.as_item_mut();
    for part in &parts[..parts.len() - 1] {
        if target.get(part).is_none() {
            target[*part] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        target = &mut target[*part];
    }
    let field = parts[parts.len() - 1];
    if let (Some(old), Some(new)) = (
        target.get(field).and_then(|i| i.as_value()),
        item.as_value_mut(),
    ) {
        *new.decor_mut() = old.decor().clone();
    }
    target[field] = item;
    let updated = doc.to_string();
    config::parse(&updated)?;
    Ok(updated)
}
fn read(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}
fn lock(path: &Path) -> Result<fs::File> {
    private_dir(path.parent().unwrap())?;
    let lock = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(path.with_file_name(format!(
            "{}.lock",
            path.file_name().unwrap().to_string_lossy()
        )))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(lock)
}
fn replace(path: &Path, source: &str, updated: &str) -> Result<()> {
    let tmp = path.with_file_name(format!(
        ".runtjik-conf-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(updated.as_bytes())?;
        f.sync_all()?;
        if let Ok(meta) = path.metadata() {
            fs::set_permissions(
                &tmp,
                fs::Permissions::from_mode(meta.permissions().mode() & 0o777),
            )?;
        }
        if read(path)? != source {
            bail!("Config changed while saving; retry the command");
        }
        fs::rename(&tmp, path)?;
        fs::File::open(path.parent().unwrap())?.sync_all()?;
        Ok(())
    })();
    let _ = fs::remove_file(tmp);
    result
}
fn path() -> std::path::PathBuf {
    let path = config::path();
    path.canonicalize().unwrap_or(path)
}
pub fn save(key: &str, text: &str) -> Result<Value> {
    let value = value(key, text)?;
    let path = path();
    let _guard = lock(&path)?;
    let source = read(&path)?;
    let updated = edit(&source, key, &value)?;
    replace(&path, &source, &updated)?;
    config::parse(&updated)
}
pub fn migrate(enable: bool) -> Result<Value> {
    let path = path();
    let _guard = lock(&path)?;
    let source = read(&path)?;
    let raw: Value = serde_json::to_value(toml::from_str::<toml::Table>(&source)?)?;
    let settings = config::parse(&source)?;
    let mut updated = source.clone();
    if enable {
        let mut enabled = strings(&settings["plugins"]["enabled"]);
        for name in strings(&config::contract()["DEFAULTS"]["plugins"]["enabled"]) {
            if !enabled.contains(&name) {
                enabled.push(name);
            }
        }
        if raw["plugins"]["enabled"] != json!(enabled) {
            updated = edit(&updated, "plugins.enabled", &json!(enabled))?;
        }
    }
    for name in strings(&config::contract()["plugin_order"]) {
        for key in strings(&config::contract()["option_order"][&name]) {
            if raw["plugin_settings"][&name].get(&key).is_none() {
                updated = edit(
                    &updated,
                    &format!("plugin_settings.{name}.{key}"),
                    &config::contract()["DEFAULTS"]["plugin_settings"][&name][&key],
                )?;
            }
        }
    }
    if updated == source {
        return Ok(json!({"changed":false,"path":path}));
    }
    let backup = if path.exists() {
        let p = path.with_file_name(format!(
            "{}.pre-plugins-{}",
            path.file_name().unwrap().to_string_lossy(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&p)?;
        f.write_all(source.as_bytes())?;
        f.sync_all()?;
        Some(p)
    } else {
        None
    };
    replace(&path, &source, &updated)?;
    Ok(json!({"changed":true,"path":path,"backup":backup}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_comments_and_rejects_invalid() {
        let src = "# header\n[ui] # settings\ncolumns = 5 # count\nview = \"grid\"\n";
        let result = edit(src, "ui.columns", &json!(6)).unwrap();
        assert!(result.contains("# header"));
        assert!(result.contains("columns = 6 # count"));
        assert!(result.contains("view = \"grid\""));
        assert!(edit(src, "ui.columns", &json!(99)).is_err());
    }
    #[test]
    fn multiline_array_and_new_table() {
        let src = "[plugins]\nenabled = [\n \"Words\", # note\n]\n";
        let edited = edit(src, "plugins.enabled", &json!(["Words", "Browser"])).unwrap();
        assert_eq!(
            config::parse(&edited).unwrap()["plugins"]["enabled"],
            json!(["Words", "Browser"])
        );
        assert_eq!(
            config::parse(&edit("", "ui.columns", &json!(7)).unwrap()).unwrap()["ui"]["columns"],
            7
        );
    }
}
