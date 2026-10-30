use crate::util::*;
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{collections::HashSet, path::Path};
pub fn defaults() -> Value {
    json!({"theme":"monochrome","style":{},"placement":{"monitors":"primary","primaryMonitor":"","edge":"b","position":"c","offsetX":0,"offsetY":-24,"margin":16,"perMonitor":{}},"timeouts":{"low":5,"normal":8,"critical":0},"pauseOnHover":true,"maxVisible":4,"maxQueued":100,"history":{"enabled":true,"maxEvents":10000,"maxAgeDays":0,"logTransient":false,"drawerLimit":100},"grouping":{"enabled":true,"windowSeconds":120},"privacy":{"lockScreen":"hide"},"stack":{"animationMs":220,"heightPercent":75},"actions":{"blacklist":[]},"telegram":{"dismissIsRead":[],"dismissReadMode":"safe"},"components":{},"handlers":[]})
}
fn number(v: &Value, name: &str, min: f64, max: f64) -> Result<()> {
    ensure!(
        v.as_f64()
            .is_some_and(|n| n.is_finite() && n >= min && n <= max),
        "{name} must be between {min} and {max}"
    );
    Ok(())
}
fn placement(p: &Value) -> Result<()> {
    ensure!(p.is_object(), "placement must be an object");
    ensure!(
        ["t", "r", "b", "l"].contains(&p.get("edge").unwrap_or(&json!("b")).as_str().unwrap_or("")),
        "invalid edge"
    );
    ensure!(
        ["l", "r", "c"].contains(
            &p.get("position")
                .unwrap_or(&json!("c"))
                .as_str()
                .unwrap_or("")
        ),
        "invalid position"
    );
    for key in ["offsetX", "offsetY"] {
        let v = p.get(key).cloned().unwrap_or(json!(0));
        if let Some(s) = v.as_str() {
            ensure!(
                regex::Regex::new(r"^-?\d+(?:\.\d+)?%$")?.is_match(s),
                "invalid offset"
            );
            number(&json!(s[..s.len() - 1].parse::<f64>()?), key, -100., 100.)?;
        } else {
            number(&v, key, -100000., 100000.)?;
        }
    }
    number(p.get("margin").unwrap_or(&json!(16)), "margin", 0., 1000.)?;
    if let Some(v) = p.get("enabled") {
        ensure!(v.is_boolean(), "enabled must be boolean")
    }
    Ok(())
}
pub fn load(root: &Path) -> Result<Value> {
    let path = config_path();
    let mut cfg = defaults();
    if path.exists() {
        let patch = read_json(&path)?;
        ensure!(patch.is_object(), "config must be an object");
        merge(&mut cfg, &patch);
    }
    for key in [
        "style",
        "placement",
        "timeouts",
        "history",
        "components",
        "grouping",
        "privacy",
        "stack",
        "actions",
        "telegram",
    ] {
        ensure!(cfg[key].is_object(), "{key} must be an object")
    }
    let rules = cfg["telegram"]["dismissIsRead"]
        .as_array()
        .context("telegram.dismissIsRead must be an array")?;
    ensure!(rules.len() <= 100, "Too many dismiss rules");
    let re = regex::Regex::new(r"^(?:-?[1-9][0-9]{0,15}|@[A-Za-z0-9_]{1,32})$")?;
    for rule in rules {
        let s = rule.as_str().context("Dismiss rule must be a string")?;
        ensure!(
            re.is_match(s)
                && (s.starts_with('@') || s.parse::<i64>()?.unsigned_abs() <= 9007199254740991),
            "Invalid dismiss rule"
        );
    }
    ensure!(
        ["safe", "through"].contains(&cfg["telegram"]["dismissReadMode"].as_str().unwrap_or("")),
        "Invalid dismissReadMode"
    );
    placement(&cfg["placement"])?;
    ensure!(
        cfg["placement"]["primaryMonitor"].is_string(),
        "primaryMonitor must be a string"
    );
    let monitors = &cfg["placement"]["monitors"];
    if monitors != "all" && monitors != "primary" {
        let a = monitors.as_array().context("Invalid monitors")?;
        ensure!(
            !a.is_empty()
                && a.iter()
                    .all(|v| v.is_string() || v.as_u64().is_some_and(|n| n > 0)),
            "Invalid monitor selector"
        );
    }
    for p in cfg["placement"]["perMonitor"]
        .as_object()
        .context("perMonitor must be an object")?
        .values()
    {
        placement(p)?;
    }
    for k in ["low", "normal", "critical"] {
        number(&cfg["timeouts"][k], k, 0., 604800.)?;
    }
    for k in ["maxVisible", "maxQueued"] {
        number(&cfg[k], k, 1., 1000.)?;
        ensure!(cfg[k].is_u64(), "{k} must be an integer");
    }
    ensure!(
        cfg["maxQueued"].as_u64() >= cfg["maxVisible"].as_u64(),
        "maxQueued must be at least maxVisible"
    );
    for k in ["maxEvents", "drawerLimit"] {
        number(&cfg["history"][k], k, 0., 1000000.)?;
        ensure!(cfg["history"][k].is_u64(), "{k} must be an integer");
    }
    number(&cfg["history"]["maxAgeDays"], "maxAgeDays", 0., 3650.)?;
    ensure!(
        cfg["history"]["maxEvents"].as_u64() != Some(0)
            || cfg["history"]["maxAgeDays"].as_f64() != Some(0.),
        "history requires bounded retention"
    );
    for v in [
        &cfg["pauseOnHover"],
        &cfg["history"]["enabled"],
        &cfg["history"]["logTransient"],
        &cfg["grouping"]["enabled"],
    ] {
        ensure!(v.is_boolean(), "flags must be boolean");
    }
    ensure!(
        ["hide", "redact", "show"].contains(&cfg["privacy"]["lockScreen"].as_str().unwrap_or("")),
        "Invalid lockScreen policy"
    );
    number(
        &cfg["grouping"]["windowSeconds"],
        "windowSeconds",
        0.,
        86400.,
    )?;
    number(&cfg["stack"]["animationMs"], "animationMs", 0., 1000.)?;
    number(&cfg["stack"]["heightPercent"], "heightPercent", 20., 95.)?;
    for hook in cfg["actions"]
        .get("nativeHooks")
        .unwrap_or(&json!([]))
        .as_array()
        .context("nativeHooks must be an array")?
    {
        for k in ["caller", "identifier"] {
            ensure!(
                hook[k].as_str().is_some_and(|s| !s.trim().is_empty()),
                "nativeHooks needs {k}"
            );
        }
        let args = hook["command"]
            .as_array()
            .context("command must be an argument array")?;
        ensure!(
            !args.is_empty()
                && args[0].as_str().is_some_and(|s| !s.is_empty())
                && args
                    .iter()
                    .all(|v| v.as_str().is_some_and(|s| !s.contains('\0'))),
            "Invalid native command"
        );
    }
    ensure!(
        cfg["actions"]["blacklist"]
            .as_array()
            .context("blacklist must be an array")?
            .iter()
            .all(|v| v.as_str().is_some_and(|s| !s.trim().is_empty())),
        "Invalid blacklist"
    );
    let rules = cfg["handlers"]
        .as_array()
        .context("handlers must be an array")?;
    ensure!(rules.len() <= 64, "Too many handlers");
    let mut ids = HashSet::new();
    let names = regex::Regex::new(r"^[a-zA-Z][a-zA-Z0-9_]*$")?;
    for rule in rules {
        let id = rule["id"].as_str().context("handler needs id")?;
        ensure!(
            !id.trim().is_empty() && ids.insert(id),
            "handlers require unique ids"
        );
        let sel = rule["match"]
            .as_object()
            .context("handler needs source selector")?;
        ensure!(
            sel.get("app")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty())
                || sel.get("telegram") == Some(&json!(true)),
            "handler needs app or telegram"
        );
        for (k, v) in sel {
            ensure!(
                ["app", "telegram", "title", "chatId"].contains(&k.as_str()),
                "Invalid handler selector"
            );
            ensure!(
                if k == "telegram" {
                    v == true
                } else {
                    v.as_str().is_some_and(|s| !s.trim().is_empty())
                },
                "Invalid handler selector"
            );
        }
        let pattern = rule["pattern"].as_str().context("handler needs pattern")?;
        ensure!(
            !pattern.is_empty() && pattern.chars().count() <= 4096,
            "Invalid pattern length"
        );
        let re = fancy_regex::Regex::new(pattern).context("invalid handlers.pattern")?;
        let card = rule["card"].as_str().context("handler needs card")?;
        ensure!(
            card == "key-value" || cfg["components"].get(card).is_some(),
            "Invalid handler card"
        );
        let captures = rule["captures"]
            .as_object()
            .context("handler needs captures")?;
        ensure!(!captures.is_empty(), "Empty captures");
        for (k, v) in captures {
            ensure!(
                names.is_match(k)
                    && !["fields", "__proto__", "constructor", "prototype"].contains(&k.as_str()),
                "Invalid capture name"
            );
            ensure!(
                v.as_u64()
                    .is_some_and(|n| n > 0 && (n as usize) < re.captures_len()),
                "Missing capture"
            );
        }
        let empty = json!([]);
        let fields = rule
            .get("fields")
            .unwrap_or(&empty)
            .as_array()
            .context("fields must be an array")?;
        ensure!(fields.len() <= 16, "Too many fields");
        for f in fields {
            ensure!(
                f["label"].as_str().is_some_and(|s| !s.trim().is_empty())
                    && f["key"].as_str().is_some_and(|s| captures.contains_key(s)),
                "Invalid field"
            );
        }
        ensure!(
            card != "key-value" || (captures.contains_key("heading") && !fields.is_empty()),
            "key-value needs heading and fields"
        );
    }
    let theme = cfg["theme"].as_str().context("theme must be a string")?;
    let theme_path = if ["monochrome", "rose-punk"].contains(&theme) {
        root.join("themes").join(format!("{theme}.json"))
    } else {
        path.parent().unwrap().join(expand(theme))
    };
    let mut style = read_json(&theme_path)?;
    ensure!(style.is_object(), "Theme must be an object");
    merge(&mut style, &cfg["style"]);
    let colors = regex::Regex::new(r"^#[0-9a-fA-F]{6}(?:[0-9a-fA-F]{2})?$")?;
    for k in [
        "background",
        "surface",
        "border",
        "text",
        "muted",
        "accent",
        "critical",
        "success",
    ] {
        ensure!(
            style[k].as_str().is_some_and(|s| colors.is_match(s)),
            "style.{k} must be a hex color"
        );
    }
    for (k, min, max) in [
        ("fontSize", 8., 48.),
        ("radius", 0., 100.),
        ("padding", 4., 64.),
        ("gap", 0., 64.),
        ("borderWidth", 0., 8.),
        ("iconSize", 16., 256.),
        ("width", 240., 1200.),
        ("maxBodyLines", 1., 30.),
    ] {
        number(&style[k], k, min, max)?;
    }
    ensure!(style["font"].is_string(), "font must be a string");
    cfg["style"] = style;
    let names = regex::Regex::new(r"^[a-zA-Z][a-zA-Z0-9_-]*$")?;
    for (k, v) in cfg["components"].as_object_mut().unwrap() {
        ensure!(names.is_match(k), "Invalid component name");
        let p = path
            .parent()
            .unwrap()
            .join(expand(
                v.as_str().context("Component path must be a string")?,
            ))
            .canonicalize()?;
        if p.extension().and_then(|s| s.to_str()) != Some("qml") || !p.is_file() {
            bail!("Component must point to a local QML file")
        }
        *v = json!(uri(&p)?);
    }
    Ok(cfg)
}
