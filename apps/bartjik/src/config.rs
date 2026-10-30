use crate::util::*;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::OnceLock,
};
pub fn contract() -> &'static Value {
    static C: OnceLock<Value> = OnceLock::new();
    C.get_or_init(|| serde_json::from_str(include_str!("contract.json")).unwrap())
}
pub fn merge(a: &Value, b: &Value) -> Value {
    let mut r = obj(a);
    for (k, v) in obj(b) {
        let v = if v.is_object() && r.get(&k).is_some_and(Value::is_object) {
            merge(&r[&k], &v)
        } else {
            v
        };
        r.insert(k, v);
    }
    Value::Object(r)
}
pub fn path() -> PathBuf {
    std::env::var_os("BARTJIK_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg("XDG_CONFIG_HOME", ".config").join("bartjik/config.json"))
}
fn object(v: &Value, n: &str) -> Result<()> {
    ensure!(v.is_object(), "{n} must be an object");
    Ok(())
}
fn number(v: &Value, n: &str, lo: f64, hi: f64) -> Result<()> {
    ensure!(
        v.as_f64()
            .is_some_and(|x| x.is_finite() && x >= lo && x <= hi),
        "{n} must be a finite number in [{lo}, {hi}]"
    );
    Ok(())
}
fn boolean(v: &Value, n: &str) -> Result<()> {
    ensure!(v.is_boolean(), "{n} must be boolean");
    Ok(())
}
fn optional(v: &Value, k: &str, f: impl FnOnce(&Value, &str) -> Result<()>) -> Result<()> {
    if let Some(x) = v.get(k) {
        f(x, k)?;
    }
    Ok(())
}
fn one(v: &Value, choices: &[&str], n: &str) -> Result<()> {
    ensure!(choices.contains(&s(v)), "invalid {n}");
    Ok(())
}
fn identifier(v: &Value) -> Result<()> {
    ensure!(
        regex::Regex::new(r"^[A-Za-z0-9_.:-]{1,100}$")
            .unwrap()
            .is_match(s(v)),
        "invalid identifier"
    );
    Ok(())
}
pub fn argv(v: &Value) -> Result<Vec<String>> {
    let a = v.as_array().context("command must be an argument array")?;
    ensure!(
        !a.is_empty()
            && a.len() <= 64
            && !s(&a[0]).is_empty()
            && a.iter().all(|v| v
                .as_str()
                .is_some_and(|x| !x.contains('\0') && x.len() <= 8192)),
        "invalid command argument array"
    );
    Ok(a.iter().map(st).collect())
}
fn actions(v: &Value) -> Result<()> {
    object(v, "actions")?;
    for (k, a) in obj(v) {
        ensure!(
            arr(&contract()["EVENTS"]).contains(&json!(k)),
            "unknown event: {k}"
        );
        if a.is_null() {
            continue;
        }
        if a.is_array() {
            argv(&a)?;
        } else if a.is_object() {
            ensure!(
                obj(&a)
                    .keys()
                    .all(|k| ["command", "detach"].contains(&k.as_str())),
                "invalid action"
            );
            optional(&a, "detach", boolean)?;
            argv(&a["command"])?;
        } else {
            ensure!(arr(&contract()["INTERNAL"]).contains(&a), "unknown action");
        }
    }
    Ok(())
}
fn dimension(v: &Value, n: &str, signed: bool) -> Result<()> {
    let lo = if signed { -100000. } else { 0. };
    if let Some(t) = v.as_str() {
        ensure!(
            regex::Regex::new(r"^-?\d+(?:\.\d+)?(?:px|%)$")
                .unwrap()
                .is_match(t),
            "{n}: use pixels or a percentage"
        );
        let x = t
            .trim_end_matches("px")
            .trim_end_matches('%')
            .parse::<f64>()?;
        number(
            &json!(x),
            n,
            lo,
            if t.ends_with('%') { 100. } else { 100000. },
        )
    } else {
        number(v, n, lo, 100000.)
    }
}
pub fn style(v: &Value) -> Result<()> {
    object(v, "style")?;
    for k in [
        "fontSize",
        "iconSize",
        "padding",
        "paddingX",
        "paddingY",
        "radius",
        "borderWidth",
        "iconTextGap",
        "itemGap",
        "moduleGap",
        "minItemSize",
        "maxTextWidth",
        "previewHeight",
    ] {
        optional(v, k, |v, n| number(v, n, 0., 1000.))?;
    }
    for k in [
        "font",
        "iconFont",
        "background",
        "surface",
        "border",
        "panelBorder",
        "text",
        "muted",
        "accent",
        "color",
    ] {
        optional(v, k, |v, n| {
            ensure!(v.is_string(), "{n} must be text");
            Ok(())
        })?;
    }
    for k in [
        "greyscaleIcons",
        "monochromeIcon",
        "animateIcons",
        "marquee",
    ] {
        optional(v, k, boolean)?;
    }
    optional(v, "iconSet", |v, n| one(v, &["proicons", "material"], n))?;
    if let Some(states) = v.get("states") {
        object(states, "states")?;
        for state in obj(states).values() {
            style(state)?;
        }
    }
    Ok(())
}
fn layout(v: &Value) -> Result<()> {
    object(v, "layout")?;
    optional(v, "centre_width", |v, n| dimension(v, n, false))?;
    if let Some(sides) = v.get("sides") {
        object(sides, "sides")?;
        for (k, p) in obj(sides) {
            ensure!(
                ["t", "r", "b", "l"].contains(&k.as_str()),
                "invalid layout side"
            );
            layout(&p)?;
        }
    }
    Ok(())
}
pub fn module(v: &Value) -> Result<()> {
    object(v, "module")?;
    ensure!(
        arr(&contract()["PROVIDERS"]).contains(&v["provider"]),
        "unknown provider"
    );
    for (k, lo, hi) in [
        ("interval", 0.1, 86400.),
        ("timeout", 0.1, 60.),
        ("maxOutput", 1024., 1048576.),
        ("hoverDelay", 0., 10000.),
        ("doubleClickInterval", 100., 1000.),
    ] {
        optional(v, k, |v, n| number(v, n, lo, hi))?;
    }
    optional(v, "actions", |v, _| actions(v))?;
    optional(v, "style", |v, _| style(v))?;
    for part in ["icon", "text", "artist", "track"] {
        if let Some(p) = v.get(part) {
            object(p, part)?;
            optional(p, "actions", |v, _| actions(v))?;
            optional(p, "style", |v, _| style(v))?;
        }
    }
    if let Some(items) = v.get("items") {
        object(items, "items")?;
        for p in obj(items).values() {
            module(&merge(&json!({"provider":v["provider"]}), p))?;
        }
    }
    match s(&v["provider"]) {
        "process" => {
            argv(&v["command"])?;
            optional(v, "mode", |v, n| one(v, &["poll", "stream"], n))?;
        }
        "icontray" => {
            if let Some(overrides) = v.get("iconOverrides") {
                object(overrides, "iconOverrides")?;
                for (k, source) in obj(overrides) {
                    let source = s(&source);
                    ensure!(!k.is_empty() && !source.is_empty(), "invalid icon override");
                    if let Some(b) = source.strip_prefix("builtin:") {
                        ensure!(
                            [
                                "googlechrome",
                                "mullvad",
                                "telegram",
                                "whatsapp",
                                "spotify",
                                "chatgpt",
                                "chatgpt-outline",
                                "telegram-outline",
                                "whatsapp-outline"
                            ]
                            .contains(&b),
                            "unknown tray artwork"
                        );
                    } else {
                        ensure!(
                            source.starts_with('/') || source.starts_with("file:/"),
                            "tray artwork must be local"
                        );
                    }
                }
            }
        }
        "workspaces" => {
            for k in ["special", "occupiedOnly", "offscreen"] {
                optional(v, k, boolean)?;
            }
            if let Some(labels) = v.get("labels") {
                object(labels, "labels")?;
                ensure!(
                    obj(labels).len() <= 128
                        && obj(labels).iter().all(|(k, v)| !k.is_empty()
                            && v.as_str()
                                .is_some_and(|s| !s.is_empty() && s.chars().count() <= 64)),
                    "invalid workspace labels"
                );
            }
            if let Some(p) = v.get("preview") {
                object(p, "preview")?;
                optional(p, "enabled", boolean)?;
                for (k, lo, hi) in [
                    ("delay", 0., 10000.),
                    ("closeDelay", 0., 5000.),
                    ("slideDuration", 0., 1000.),
                    ("width", 160., 800.),
                    ("refreshInterval", 33., 5000.),
                    ("maxWindows", 1., 32.),
                ] {
                    optional(p, k, |v, n| number(v, n, lo, hi))?;
                }
            }
            optional(v, "label", |v, n| {
                one(v, &["name", "circles", "circles_name"], n)
            })?;
            optional(v, "appearance", |v, n| {
                one(v, &["plain", "chips", "separators", "ring"], n)
            })?;
        }
        "datetime" => {
            if truth(&v["timezone"]) {
                s(&v["timezone"])
                    .parse::<chrono_tz::Tz>()
                    .context("invalid timezone")?;
            }
            if let Some(cities) = v.get("worldClocks") {
                ensure!(
                    cities.is_array() && arr(cities).len() <= 24,
                    "invalid worldClocks"
                );
                for c in arr(cities) {
                    ensure!(
                        c["label"].is_string() && c["timezone"].is_string(),
                        "invalid world clock"
                    );
                    s(&c["timezone"])
                        .parse::<chrono_tz::Tz>()
                        .context("invalid timezone")?;
                }
            }
        }
        "mullvad" => {
            optional(v, "regionFlag", boolean)?;
            if let Some(c) = v.get("quickCountries") {
                let re = regex::Regex::new("^[a-z]{2}$")?;
                ensure!(
                    c.is_array()
                        && arr(c).len() <= 8
                        && arr(c).iter().all(|x| re.is_match(s(x)))
                        && arr(c).iter().map(s).collect::<HashSet<_>>().len() == arr(c).len(),
                    "invalid quickCountries"
                );
            }
        }
        "openrouter" => {
            optional(v, "currencySymbol", |v, _| {
                ensure!(v.is_string(), "currencySymbol must be text");
                Ok(())
            })?;
        }
        "crypto" => {
            if let Some(a) = v.get("assets") {
                ensure!(a.is_array(), "assets must be array");
                for a in arr(a) {
                    ensure!(
                        regex::Regex::new("^[A-Za-z0-9]+$")?.is_match(s(&a["symbol"])),
                        "invalid crypto symbol"
                    );
                    for k in ["price_precision", "change_precision", "volume_precision"] {
                        optional(a, k, |v, n| number(v, n, 0., 12.))?;
                    }
                }
            }
        }
        _ => {}
    }
    if let Some(p) = v.get("panel") {
        object(p, "panel")?;
        optional(p, "hover", boolean)?;
        for k in ["delay", "closeDelay"] {
            optional(p, k, |v, n| number(v, n, 0., 10000.))?;
        }
    }
    Ok(())
}
fn bar(v: &Value, modules: &Value) -> Result<()> {
    object(v, "bar")?;
    ensure!(
        v.get("layout").is_none() && v.get("centre_width").is_none(),
        "column geometry belongs to global/perMonitor layout"
    );
    one(&v["side"], &["t", "r", "b", "l"], "side")?;
    for k in ["position", "alignment"] {
        one(&v[k], &["l", "c", "r"], k)?;
    }
    let selectors = &v["monitors"];
    ensure!(
        selectors == "all"
            || (selectors.is_array()
                && !arr(selectors).is_empty()
                && arr(selectors)
                    .iter()
                    .all(|v| v.is_string() || v.as_u64().is_some_and(|n| n >= 1))),
        "invalid monitor selectors"
    );
    one(&v["overflow"], &["clip", "centre"], "overflow")?;
    one(
        &v["activationEdge"],
        &["side", "t", "r", "b", "l"],
        "activationEdge",
    )?;
    if v["screenGutter"] != "edgeOffset" {
        dimension(&v["screenGutter"], "screenGutter", false)?;
    }
    dimension(&v["keepVisibleDistance"], "keepVisibleDistance", false)?;
    for k in ["offsetX", "offsetY"] {
        dimension(&v[k], k, true)?;
    }
    for k in [
        "padding",
        "islandGap",
        "stackGap",
        "thickness",
        "verticalWidth",
        "hideDelay",
        "activationDelay",
        "fadeDuration",
        "revealSize",
    ] {
        number(&v[k], k, 0., 10000.)?;
    }
    for k in ["enabled", "visible", "reserve", "autoHide", "syncMonitors"] {
        boolean(&v[k], k)?;
    }
    one(
        &v["layer"],
        &["background", "bottom", "top", "overlay"],
        "layer",
    )?;
    optional(v, "style", |v, _| style(v))?;
    ensure!(v["islands"].is_array(), "islands must be an array");
    let mut ids = HashSet::new();
    for island in arr(&v["islands"]) {
        identifier(&island["id"])?;
        ensure!(ids.insert(s(&island["id"])), "duplicate island ID");
        let ms = &island["modules"];
        ensure!(
            ms.is_array()
                && arr(ms)
                    .iter()
                    .all(|m| m.is_string() && modules.get(s(m)).is_some()),
            "unknown module"
        );
        ensure!(
            arr(ms).iter().map(s).collect::<HashSet<_>>().len() == arr(ms).len(),
            "duplicate module in island"
        );
        optional(island, "style", |v, _| style(v))?;
    }
    Ok(())
}
fn theme(v: &Value) -> Result<Value> {
    let name = v.as_str().context("theme must be text")?;
    let p = if !name.contains('/') && !name.ends_with(".json") {
        root().join("themes").join(format!("{name}.json"))
    } else {
        path().parent().unwrap().join(name)
    };
    read(p)
}
pub fn load() -> Result<Value> {
    load_path(path())
}
fn load_path(p: PathBuf) -> Result<Value> {
    let patch = if p.exists() { read(p)? } else { json!({}) };
    object(&patch, "config")?;
    let mut cfg = merge(&contract()["DEFAULTS"], &patch);
    if patch.get("modules").is_some() {
        cfg["modules"] = patch["modules"].clone();
    }
    for k in ["style", "layout", "perMonitor", "modules"] {
        object(&cfg[k], k)?;
    }
    cfg["_styleOverrides"] = cfg["style"].clone();
    cfg["style"] = merge(&theme(&cfg["theme"])?, &cfg["style"]);
    style(&cfg["style"])?;
    layout(&cfg["layout"])?;
    for (k, m) in obj(&cfg["modules"]) {
        identifier(&json!(k))?;
        module(&m)?;
    }
    ensure!(
        cfg["bars"].is_array() && arr(&cfg["bars"]).len() <= 64,
        "invalid bars"
    );
    let mut ids = HashSet::new();
    let mut bars = vec![];
    for b in arr(&cfg["bars"]) {
        identifier(&b["id"])?;
        ensure!(ids.insert(s(&b["id"])), "duplicate bar ID");
        let b = merge(&contract()["BAR"], b);
        bar(&b, &cfg["modules"])?;
        object(&b["perMonitor"], "bar.perMonitor")?;
        for p in obj(&b["perMonitor"]).values() {
            object(p, "bar override")?;
            ensure!(
                p.get("id").is_none() && p.get("perMonitor").is_none(),
                "bar override cannot change identity"
            );
            bar(&merge(&b, p), &merge(&cfg["modules"], &p["modules"]))?;
        }
        bars.push(b);
    }
    cfg["bars"] = json!(bars);
    for p in obj(&cfg["perMonitor"]).values() {
        object(p, "perMonitor")?;
        layout(p.get("layout").unwrap_or(&json!({})))?;
        style(p.get("style").unwrap_or(&json!({})))?;
        for (k, m) in obj(&p["modules"]) {
            module(&merge(&cfg["modules"][k], &m))?;
        }
    }
    for (i, k) in obj(&cfg["perMonitor"]).keys().enumerate() {
        resolve(
            &cfg,
            &json!({"name":k,"width":100000,"height":100000,"scale":1}),
            i,
            true,
        )?;
    }
    Ok(cfg)
}
fn monitor_patch(p: &Value, screen: &Value, i: usize) -> Value {
    merge(&p[(i + 1).to_string()], &p[s(&screen["name"])])
}
pub fn resolve(cfg: &Value, screen: &Value, i: usize, validate: bool) -> Result<Vec<Value>> {
    let patch = monitor_patch(&cfg["perMonitor"], screen, i);
    let mut merged = merge(cfg, &patch);
    if patch.get("theme").is_some() {
        merged["style"] = merge(
            &merge(&theme(&patch["theme"])?, &cfg["_styleOverrides"]),
            &patch["style"],
        );
    }
    style(&merged["style"])?;
    let mut result = vec![];
    let mut stacks: HashMap<String, f64> = HashMap::new();
    for orig in arr(&cfg["bars"]) {
        let selectors = &orig["monitors"];
        if selectors != "all"
            && !arr(selectors).contains(&screen["name"])
            && !arr(selectors).contains(&json!(i + 1))
        {
            continue;
        }
        let mut b = merge(orig, &monitor_patch(&orig["perMonitor"], screen, i));
        let modules = merge(&merged["modules"], &b["modules"]);
        for m in obj(&modules).values() {
            module(m)?;
        }
        bar(&b, &modules)?;
        if b["enabled"] == false || patch["enabled"] == false {
            continue;
        }
        let lay = merge(&merged["layout"], &merged["layout"]["sides"][s(&b["side"])]);
        let horizontal = ["t", "b"].contains(&s(&b["side"]));
        let span = num(&screen[if horizontal { "width" } else { "height" }]);
        if !validate && lay.get("centre_width").is_some() {
            let v = &lay["centre_width"];
            let px = if s(v).ends_with('%') {
                s(v).trim_end_matches('%').parse::<f64>()? * span / 100.
            } else {
                st(v).trim_end_matches("px").parse::<f64>()? / num(&screen["scale"])
            };
            ensure!(px <= span, "centre_width exceeds monitor span");
        }
        let group = format!("{}/{}", s(&b["side"]), s(&b["position"]));
        let depth = *stacks.get(&group).unwrap_or(&0.);
        stacks.insert(
            group,
            depth
                + num(&b[if horizontal {
                    "thickness"
                } else {
                    "verticalWidth"
                }])
                + num(&b["stackGap"]),
        );
        b["key"] = json!(format!("{}/{}", s(&screen["name"]), s(&b["id"])));
        b["screen"] = screen["name"].clone();
        b["layout"] = lay;
        b["style"] = merge(&merged["style"], &b["style"]);
        b["stackOffset"] = json!(depth);
        b["moduleConfigs"] = modules;
        result.push(b);
    }
    Ok(result)
}
pub fn provider_config(m: &Value) -> Value {
    let mut m = obj(m);
    for k in [
        "style",
        "actions",
        "icon",
        "text",
        "artist",
        "track",
        "items",
        "hoverDelay",
        "doubleClickInterval",
    ] {
        m.remove(k);
    }
    json!(m)
}
pub fn provider_key(m: &Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(provider_config(m).to_string().as_bytes())
    )[..20]
        .into()
}

/// Validate shipped configurations and the pinned, licensed icon font.
pub fn check_assets() -> Result<()> {
    for entry in std::fs::read_dir(root().join("examples"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|x| x == "json") {
            load_path(path)?;
        }
    }
    for entry in std::fs::read_dir(root().join("themes"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|x| x == "json") {
            read(path)?;
        }
    }
    let fonts = root().join("qml/fonts");
    let provenance = read(fonts.join("provenance.json"))?;
    let bytes = std::fs::read(fonts.join(s(&provenance["file"])))?;
    ensure!(
        bytes.len() as u64 == provenance["size"].as_u64().unwrap_or(0)
            && format!("{:x}", Sha256::digest(&bytes)) == s(&provenance["sha256"]),
        "Bundled font does not match its pinned provenance"
    );
    ensure!(
        fonts.join("LICENSE.txt").is_file(),
        "Bundled font license is missing"
    );
    Ok(())
}
