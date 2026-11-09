//! Display layout, advertised modes, and independently guarded configuration changes.
use crate::{actions::FileLock, util::*, workspaces};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Write, path::PathBuf, time::Duration};

const PREFIX: &str = "-- bartjik-monitors-v1 ";
const FIELDS: &[&str] = &[
    "enabled",
    "mode",
    "x",
    "y",
    "scale",
    "transform",
    "vrr",
    "cm",
    "mirror",
];
fn settings_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("bartjik/monitors.lua")
}
fn pending_path() -> PathBuf {
    xdg("XDG_RUNTIME_DIR", ".cache").join("bartjik-monitors-pending.json")
}
fn ready_path(token: &str) -> PathBuf {
    xdg("XDG_RUNTIME_DIR", ".cache").join(format!("bartjik-monitors-{token}.ready"))
}
fn catalogue_path() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join("bartjik/monitors.json")
}
fn instance() -> String {
    std::env::var("HYPRLAND_INSTANCE_SIGNATURE").unwrap_or_default()
}
fn session_alive(v: &Value) -> bool {
    !s(&v["instance"]).is_empty()
        && xdg("XDG_RUNTIME_DIR", ".cache")
            .join("hypr")
            .join(s(&v["instance"]))
            .join(".socket.sock")
            .exists()
}
fn read_settings() -> Result<Value> {
    let path = settings_path();
    if !path.exists() {
        return Ok(json!({}));
    }
    let content = std::fs::read_to_string(path)?;
    let data = content
        .lines()
        .next()
        .and_then(|l| l.strip_prefix(PREFIX))
        .context("Invalid saved display settings")?;
    let v: Value = serde_json::from_str(data)?;
    ensure!(v.is_object(), "Invalid saved display settings");
    Ok(v)
}
fn mode(value: &str) -> Option<(u64, u64, f64)> {
    let re = regex::Regex::new(r"^(\d+)x(\d+)@(\d+(?:\.\d+)?)(?:Hz)?$").unwrap();
    let c = re.captures(value)?;
    Some((c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?))
}
fn canonical(value: &str) -> String {
    mode(value)
        .map(|(w, h, r)| format!("{w}x{h}@{r:.2}"))
        .unwrap_or(value.into())
}
fn logical(v: &Value) -> (f64, f64) {
    let (mut w, mut h, _) = mode(s(&v["mode"])).unwrap_or((0, 0, 0.));
    if v["transform"].as_u64().unwrap_or(0) % 2 == 1 {
        std::mem::swap(&mut w, &mut h);
    }
    let scale = num(&v["scale"]).max(0.1);
    (w as f64 / scale, h as f64 / scale)
}
fn default_settings() -> Value {
    json!({"enabled":true,"mode":"preferred","x":0,"y":0,"scale":1,"transform":0,"vrr":-1,"cm":"auto","mirror":""})
}
// Read literal declarations only. Never execute the user's configuration to inspect it.
fn configured(text: &str) -> BTreeMap<String, Value> {
    let blocks = regex::Regex::new(r"(?s)hl\.monitor\s*\(\s*\{([^}]+)\}\s*\)").unwrap();
    let fields =
        regex::Regex::new(r#"(?m)^\s*(\w+)\s*=\s*("[^"]*"|true|false|-?\d+(?:\.\d+)?)\s*,?"#)
            .unwrap();
    let uncommented = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut out: BTreeMap<String, Value> = BTreeMap::new();
    for block in blocks.captures_iter(&uncommented) {
        let mut values = json!({});
        for c in fields.captures_iter(&block[1]) {
            if let Ok(v) = serde_json::from_str::<Value>(&c[2]) {
                values[&c[1]] = v;
            }
        }
        let name = s(&values["output"]);
        if !valid_output(name) {
            continue;
        }
        let row = out.entry(name.into()).or_insert_with(default_settings);
        for field in ["mode", "scale", "transform", "vrr", "cm", "mirror"] {
            if !values[field].is_null() {
                row[field] = values[field].clone();
            }
        }
        if let Some(disabled) = values["disabled"].as_bool() {
            row["enabled"] = json!(!disabled);
        }
        if let Some((x, y)) = s(&values["position"])
            .split_once('x')
            .and_then(|(x, y)| Some((x.parse::<i64>().ok()?, y.parse::<i64>().ok()?)))
        {
            row["x"] = json!(x);
            row["y"] = json!(y);
        } else if values["position"].is_string() {
            row["autoPosition"] = values["position"].clone();
        }
    }
    // Resolve the common relative declarations for disconnected/disabled displays.
    let anchor = out.values().find(|v| v["autoPosition"].is_null()).cloned();
    if let Some(a) = anchor {
        let (aw, ah) = logical(&a);
        for v in out.values_mut() {
            let (w, h) = logical(v);
            match s(&v["autoPosition"]) {
                "auto-center-left" => {
                    v["x"] = json!((num(&a["x"]) - w) as i64);
                    v["y"] = json!((num(&a["y"]) + (ah - h) / 2.) as i64);
                }
                "auto-right" | "auto-center-right" => {
                    v["x"] = json!((num(&a["x"]) + aw) as i64);
                    v["y"] = a["y"].clone();
                }
                _ => {}
            }
            v.as_object_mut().unwrap().remove("autoPosition");
        }
    }
    out
}
fn valid_output(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}
fn merge(a: &Value, b: &Value) -> Value {
    crate::config::merge(a, b)
}
fn mirror_source(m: &Value, live: &Value) -> String {
    let source = &m["mirrorOf"];
    if source.is_null() || source == "none" || source == "" {
        return String::new();
    }
    arr(live)
        .iter()
        .find(|r| {
            r["name"] == *source
                || (!r["id"].is_null() && (r["id"] == *source || st(&r["id"]) == s(source)))
        })
        .map(|r| st(&r["name"]))
        .unwrap_or_default()
}
fn live_settings(m: &Value, base: &Value, live: &Value) -> Value {
    let mut v = base.clone();
    v["enabled"] = json!(!truth(&m["disabled"]));
    if !truth(&m["disabled"]) && num(&m["width"]) > 0. {
        v["mirror"] = json!(mirror_source(m, live));
        v["mode"] = json!(format!(
            "{}x{}@{:.2}",
            m["width"],
            m["height"],
            num(&m["refreshRate"])
        ));
        for k in ["scale", "transform"] {
            v[k] = m[k].clone();
        }
        // A mirror has its source's live origin. Retain its independent placement
        // so returning to Extend restores the user's arrangement.
        if s(&v["mirror"]).is_empty() {
            for k in ["x", "y"] {
                v[k] = m[k].clone();
            }
        }
    }
    v
}
fn catalogue(
    live: &Value,
    configured: &BTreeMap<String, Value>,
    remembered: &Value,
    saved: &Value,
) -> Vec<Value> {
    let mut known: BTreeMap<String, Value> = obj(remembered).into_iter().collect();
    for (name, v) in configured {
        known.entry(name.clone()).or_insert(json!({"settings":v}));
    }
    for m in arr(live) {
        if valid_output(s(&m["name"])) {
            known.entry(s(&m["name"]).into()).or_insert(json!({}));
        }
    }
    let mut rows = vec![];
    for (name, old) in known {
        let actual = arr(live).iter().find(|m| m["name"] == name);
        let base = merge(
            &merge(&default_settings(), &old["settings"]),
            configured.get(&name).unwrap_or(&json!({})),
        );
        let base = merge(&base, &saved[&name]);
        let mut settings = actual
            .map(|m| live_settings(m, &base, live))
            .unwrap_or(base);
        settings["mode"] = json!(canonical(s(&settings["mode"])));
        let metadata = actual.unwrap_or(&old);
        let mut modes: Vec<String> = arr(&metadata["availableModes"])
            .iter()
            .filter_map(|m| mode(s(m)).map(|_| canonical(s(m))))
            .collect();
        modes.dedup();
        modes.sort();
        modes.dedup();
        if settings["mode"] == "preferred" {
            if let Some(first) = arr(&metadata["availableModes"]).first() {
                settings["mode"] = json!(canonical(s(first)));
            }
        }
        let label = if !s(&metadata["model"]).is_empty() {
            s(&metadata["model"])
        } else {
            &name
        };
        let (width, height) = logical(&settings);
        rows.push(json!({"name":name,"label":label,"connected":actual.is_some(),"configured":configured.contains_key(&name),"enabled":actual.is_some() && truth(&settings["enabled"]),"settings":settings,"width":width,"height":height,"availableModes":modes,"model":metadata["model"],"description":metadata["description"],"hardwareDetails":metadata["hardwareDetails"],"vrrActive":metadata["vrr"],"colour":metadata["colorManagementPreset"]}));
    }
    rows
}
pub fn pending() -> Value {
    let v = read(pending_path()).unwrap_or(Value::Null);
    if !v.is_object() || v["instance"] != instance() {
        return Value::Null;
    }
    json!({"token":v["token"],"deadline":v["deadline"],"error":v["error"],"phase":v["phase"]})
}
async fn data() -> Result<Value> {
    let live = workspaces::query("monitors all").await?;
    ensure!(live.is_array(), "Monitor information unavailable");
    let base = configured(
        &std::fs::read_to_string(xdg("XDG_CONFIG_HOME", ".config").join("hypr/monitors.lua"))
            .unwrap_or_default(),
    );
    let old = read(catalogue_path()).unwrap_or(json!({}));
    let saved = read_settings()?;
    let rows = catalogue(&live, &base, &old, &saved);
    let cache: serde_json::Map<String, Value> = rows
        .iter()
        .map(|r| (s(&r["name"]).into(), r.clone()))
        .collect();
    if json!(cache) != old {
        save(&catalogue_path(), &json!(cache))?;
    }
    let revision = revision(&rows);
    Ok(
        json!({"title":"Displays","rows":[],"monitors":rows,"revision":revision,"pending":pending()}),
    )
}
fn revision(rows: &[Value]) -> String {
    use sha2::{Digest, Sha256};
    let value: Vec<_> = rows
        .iter()
        .map(|r| json!([r["name"], r["connected"], r["settings"]]))
        .collect();
    format!("{:x}", Sha256::digest(serde_json::to_vec(&value).unwrap()))
}
pub async fn snapshot(options: &Value) -> Result<Value> {
    let mut panel = data().await?;
    let tv = options["tv"].as_str().unwrap_or("HDMI-A-1");
    panel["tv"] = json!(tv);
    panel["mirrorOutput"] = json!(options["mirrorOutput"].as_str().unwrap_or("DP-2"));
    let active = arr(&panel["monitors"])
        .iter()
        .any(|r| r["name"] == tv && truth(&r["enabled"]));
    let mut row = item(
        "layout",
        None,
        Some(if active {
            "sports_esports"
        } else {
            "desktop_windows"
        }),
        "symbol",
        json!({"active":active}),
        if active { "Game Mode" } else { "Desktop Mode" },
        json!({"leftClick":"monitor.toggle","rightClick":"monitor.panel","hoverEnter":"monitor.panel"}),
    );
    row["panel"] = panel;
    Ok(row)
}
fn settings_lua(settings: &Value) -> Result<String> {
    let mut out = format!("{PREFIX}{}\nreturn {{\n", serde_json::to_string(settings)?);
    for (name, fields) in obj(settings) {
        out.push_str(&format!("    {},\n", rule(&name, &fields)?));
    }
    out.push_str("}\n");
    Ok(out)
}
fn save_settings(settings: &Value) -> Result<()> {
    let path = settings_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    file.write_all(settings_lua(settings)?.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
fn rule(name: &str, fields: &Value) -> Result<String> {
    ensure!(valid_output(name), "Invalid display connector");
    let mut parts = vec![format!("output={}", workspaces::lua(name))];
    for (key, v) in obj(fields) {
        ensure!(FIELDS.contains(&key.as_str()), "Unknown display setting");
        match key.as_str() {
            "enabled" => {
                ensure!(v.is_boolean(), "Invalid enabled value");
                parts.push(format!("disabled={}", !truth(&v)));
            }
            "x" | "y" => {}
            "mode" | "cm" | "mirror" => {
                ensure!(v.is_string(), "Invalid display setting");
                parts.push(format!("{key}={}", workspaces::lua(s(&v))));
            }
            _ => {
                ensure!(v.is_number(), "Invalid display setting");
                parts.push(format!("{key}={v}"));
            }
        }
    }
    if !fields["x"].is_null() || !fields["y"].is_null() {
        ensure!(
            fields["x"].is_i64() && fields["y"].is_i64(),
            "Position needs both coordinates"
        );
        parts.push(format!(
            "position={}",
            workspaces::lua(&format!("{}x{}", fields["x"], fields["y"]))
        ));
    }
    Ok(format!("{{{}}}", parts.join(",")))
}
fn rules_lua(rules: &Value) -> Result<String> {
    // Enable/update outputs before disabling any, keeping a usable screen throughout.
    let mut entries: Vec<_> = obj(rules).into_iter().collect();
    entries.sort_by_key(|(_, v)| v["enabled"] == false);
    entries
        .iter()
        .map(|(n, v)| Ok(format!("hl.monitor({})", rule(n, v)?)))
        .collect::<Result<Vec<_>>>()
        .map(|v| v.join(";"))
}
async fn apply_rules(rules: &Value) -> Result<()> {
    let mut base = rules.clone();
    let mut mirrors = json!({});
    for (name, fields) in obj(rules) {
        if !s(&fields["mirror"]).is_empty() {
            base[&name]["mirror"] = json!("");
            mirrors[&name] = json!({"mirror":fields["mirror"]});
        }
    }
    apply_lua(&base).await?;
    if !obj(&mirrors).is_empty() {
        // The source must be online before a mirror is attached. This also lets
        // a single preview swap the direction of an existing mirror safely.
        wait_applied(&base).await?;
        apply_lua(&mirrors).await?;
    }
    Ok(())
}
async fn apply_lua(rules: &Value) -> Result<()> {
    let lua = rules_lua(rules)?;
    if lua.is_empty() {
        return Ok(());
    }
    let response = cmd(&["hyprctl", "eval", &lua]).await?;
    ensure!(
        response.trim() == "ok",
        "Hyprland rejected the display settings: {}",
        response.chars().take(180).collect::<String>()
    );
    Ok(())
}
fn plan(panel: &Value, request: &Value, saved: &Value) -> Result<(Value, Value, Value)> {
    ensure!(
        request["revision"] == panel["revision"],
        "Displays changed while you were editing. Reset and try again."
    );
    let edits = request["edits"]
        .as_array()
        .context("Invalid display changes")?;
    ensure!(
        !edits.is_empty() && edits.len() <= 16,
        "Choose a display setting to change"
    );
    let rows = arr(&panel["monitors"]);
    let mut after = saved.clone();
    let mut apply = json!({});
    let mut before = json!({});
    for edit in edits {
        let name = s(&edit["name"]);
        ensure!(apply[name].is_null(), "Duplicate display change");
        let current = rows
            .iter()
            .find(|r| r["name"] == name && truth(&r["connected"]))
            .context("Display disconnected. Reconnect it before changing its settings.")?;
        ensure!(
            current["configured"] == true,
            "Add an explicit connector rule in hypr/monitors.lua before editing this display"
        );
        let patch = edit["settings"]
            .as_object()
            .context("Invalid display settings")?;
        ensure!(
            !patch.is_empty() && patch.keys().all(|k| FIELDS.contains(&k.as_str())),
            "Unknown display setting"
        );
        let mut next = merge(&current["settings"], &edit["settings"]);
        ensure!(next["enabled"].is_boolean(), "Invalid enabled value");
        ensure!(
            next["mirror"].is_string()
                && (s(&next["mirror"]).is_empty() || valid_output(s(&next["mirror"]))),
            "Invalid mirror source"
        );
        ensure!(next["mirror"] != name, "A display cannot mirror itself");
        let (w, h, _) = mode(s(&next["mode"])).context("Choose an advertised display mode")?;
        if patch.contains_key("mode") || (next["enabled"] == true && current["enabled"] == false) {
            ensure!(
                arr(&current["availableModes"])
                    .iter()
                    .any(|m| *m == next["mode"]),
                "That resolution or refresh rate is no longer available"
            );
        }
        let scale = num(&next["scale"]);
        ensure!(
            (0.5..=4.).contains(&scale)
                && [w, h]
                    .iter()
                    .all(|d| (*d as f64 / scale - (*d as f64 / scale).round()).abs() < 0.01),
            "Choose a scale that fits the selected resolution"
        );
        ensure!(
            next["transform"].as_u64().is_some_and(|n| n <= 7),
            "Invalid display rotation"
        );
        ensure!(
            ["x", "y"].iter().all(|k| next[*k]
                .as_i64()
                .is_some_and(|n| (-32768..=32768).contains(&n))),
            "Position is outside the desktop range"
        );
        ensure!(
            next["vrr"].as_i64().is_some_and(|n| (-1..=2).contains(&n)),
            "Invalid adaptive-sync setting"
        );
        if patch.contains_key("vrr") && num(&next["vrr"]) > 0. {
            ensure!(
                current["hardwareDetails"]["vrrCapable"] == true,
                "This display does not report adaptive-sync support"
            );
        }
        ensure!(
            !patch.contains_key("cm") || ["auto", "srgb", "hdr"].contains(&s(&next["cm"])),
            "Invalid colour mode"
        );
        if patch.contains_key("cm") && next["cm"] == "hdr" {
            ensure!(
                current["hardwareDetails"]["hdr"] == true,
                "This display does not report HDR support"
            );
        }
        let mut changes = edit["settings"].clone();
        // Hyprland needs both coordinates; disabled outputs need their saved mode/geometry.
        if patch.contains_key("x")
            || patch.contains_key("y")
            || patch.contains_key("mirror")
            || (next["enabled"] == true && current["enabled"] == false)
        {
            for key in ["x", "y"] {
                changes[key] = next[key].clone();
            }
        }
        if next["enabled"] == true && current["enabled"] == false {
            for key in ["mode", "scale", "transform"] {
                changes[key] = next[key].clone();
            }
        }
        let mut restore = json!({});
        for key in obj(&changes).keys() {
            restore[key] = current["settings"][key].clone();
        }
        // Restore mode/placement as well when re-enabling an output on rollback.
        if current["enabled"] == true && next["enabled"] == false {
            restore = current["settings"].clone();
        }
        apply[name] = changes.clone();
        before[name] = restore;
        after[name] = merge(&saved[name], &changes);
        next.as_object_mut()
            .unwrap()
            .retain(|k, _| FIELDS.contains(&k.as_str()));
    }
    let final_settings: BTreeMap<_, _> = rows
        .iter()
        .filter(|r| truth(&r["connected"]))
        .map(|r| (st(&r["name"]), merge(&r["settings"], &apply[s(&r["name"])])))
        .collect();
    for value in final_settings
        .values()
        .filter(|v| truth(&v["enabled"]) && !s(&v["mirror"]).is_empty())
    {
        let source = final_settings
            .get(s(&value["mirror"]))
            .context("Connect the mirror source first")?;
        ensure!(
            truth(&source["enabled"]),
            "Enable the mirror source, or return its copies to Extend, before applying"
        );
        ensure!(s(&source["mirror"]).is_empty(), "A mirror source must be an independent display; mirror chains and cycles are not supported");
    }
    ensure!(
        final_settings
            .values()
            .any(|v| truth(&v["enabled"]) && s(&v["mirror"]).is_empty()),
        "Keep at least one connected display on"
    );
    Ok((apply, before, after))
}
fn applied(live: &Value, rules: &Value) -> bool {
    obj(rules).iter().all(|(name, fields)| {
        let Some(row) = arr(live).iter().find(|r| r["name"] == *name) else {
            return fields["enabled"] == false;
        };
        if fields["enabled"] == false {
            return truth(&row["disabled"]);
        }
        if truth(&row["disabled"]) {
            return fields["enabled"] != true;
        }
        for (key, v) in obj(fields) {
            let good = match key.as_str() {
                "enabled" => !truth(&row["disabled"]),
                "mode" => mode(s(&v)).is_some_and(|(w, h, r)| {
                    row["width"] == w
                        && row["height"] == h
                        && (num(&row["refreshRate"]) - r).abs() < 0.04
                }),
                "scale" => (num(&row["scale"]) - num(&v)).abs() < 0.01,
                "x" | "y" => !mirror_source(row, live).is_empty() || row[&key] == v,
                "transform" => row[&key] == v,
                "mirror" => mirror_source(row, live) == s(&v),
                "cm" => v == "auto" || row["colorManagementPreset"] == v,
                _ => true, // VRR is a policy; its active state depends on the current application.
            };
            if !good {
                return false;
            }
        }
        true
    })
}
async fn wait_applied(rules: &Value) -> Result<()> {
    for _ in 0..25 {
        if let Ok(live) = workspaces::query("monitors all").await {
            if applied(&live, rules) {
                return Ok(());
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    anyhow::bail!("The display did not accept those settings; restoring the previous layout")
}
async fn rollback(v: &Value) -> Result<&'static str> {
    let live = workspaces::query("monitors all").await?;
    let mut restore = v["before"].clone();
    // A hot-unplug during preview must not turn off the only surviving screen.
    let usable = arr(&live).iter().any(|m| {
        let enabled = &restore[s(&m["name"])]["enabled"];
        enabled == true || (enabled.is_null() && !truth(&m["disabled"]))
    });
    if !usable {
        if let Some(m) = arr(&live).iter().find(|m| !truth(&m["disabled"])) {
            restore[s(&m["name"])]["enabled"] = json!(true);
        }
    }
    // Disconnected outputs cannot acknowledge a rule; the saved defaults restore
    // them on reconnect. Apply only to outputs that still exist.
    restore
        .as_object_mut()
        .unwrap()
        .retain(|name, _| arr(&live).iter().any(|m| m["name"] == *name));
    for (name, fields) in obj(&restore) {
        let source = s(&fields["mirror"]);
        if !source.is_empty()
            && !arr(&live).iter().any(|m| {
                m["name"] == source
                    && (restore[source]["enabled"] == true
                        || (restore[source]["enabled"].is_null() && !truth(&m["disabled"])))
            })
        {
            restore[&name]["mirror"] = json!("");
        }
    }
    apply_rules(&restore).await?;
    wait_applied(&restore).await?;
    std::fs::remove_file(pending_path())?;
    let _ = std::fs::remove_file(ready_path(s(&v["token"])));
    Ok(if usable {
        "Previous layout restored"
    } else {
        "Previous display disconnected; kept the remaining screen on"
    })
}
pub async fn decide(token: &str, decision: &str) -> Result<String> {
    let _lock = FileLock::acquire("bartjik-monitors.lock").await?;
    let mut v = read(pending_path()).context("The display preview has already ended")?;
    ensure!(v["token"] == token, "The display preview has already ended");
    if decision == "keep" {
        ensure!(
            now() < num(&v["deadline"]) && v["phase"] == "confirm",
            "The display preview has expired"
        );
        wait_applied(&v["apply"]).await?;
        ensure!(
            now() < num(&v["deadline"]),
            "The display preview has expired"
        );
        v["phase"] = json!("keeping");
        save(&pending_path(), &v)?;
        save_settings(&v["after"])?;
        std::fs::remove_file(pending_path())?;
        let _ = std::fs::remove_file(ready_path(token));
        Ok("Display settings saved".into())
    } else {
        ensure!(decision == "revert", "Invalid display decision");
        v["phase"] = json!("restoring");
        save(&pending_path(), &v)?;
        Ok(rollback(&v).await?.into())
    }
}
pub async fn guard(token: &str) -> Result<()> {
    ensure!(
        !token.is_empty()
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "Invalid preview token"
    );
    let first = read(pending_path())?;
    ensure!(first["token"] == token, "Preview has ended");
    std::fs::write(ready_path(token), b"ready")?;
    loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let Ok(v) = read(pending_path()) else {
            return Ok(());
        };
        if v["token"] != token {
            return Ok(());
        }
        if !session_alive(&v) {
            let _lock = FileLock::acquire("bartjik-monitors.lock").await?;
            if read(pending_path())
                .ok()
                .is_some_and(|p| p["token"] == token)
            {
                std::fs::remove_file(pending_path())?;
            }
            let _ = std::fs::remove_file(ready_path(token));
            return Ok(());
        }
        if now() < num(&v["deadline"]) {
            continue;
        }
        let _lock = FileLock::acquire("bartjik-monitors.lock").await?;
        let Ok(mut v) = read(pending_path()) else {
            return Ok(());
        };
        if v["token"] != token {
            return Ok(());
        }
        // A crash after the atomic settings write must not undo a confirmed layout.
        if v["phase"] == "keeping" && read_settings().ok().as_ref() == Some(&v["after"]) {
            std::fs::remove_file(pending_path())?;
            let _ = std::fs::remove_file(ready_path(token));
            return Ok(());
        }
        v["phase"] = json!("restoring");
        save(&pending_path(), &v)?;
        match rollback(&v).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                v["error"] = json!(format!("Restoring displays: {e}"));
                save(&pending_path(), &v)?;
            }
        }
    }
}
pub async fn action(name: &str, selection: &str, options: &Value) -> Result<String> {
    let _lock = FileLock::acquire("bartjik-monitors.lock").await?;
    if let Ok(stale) = read(pending_path()) {
        if stale["instance"] != instance() && !session_alive(&stale) {
            std::fs::remove_file(pending_path())?;
            let _ = std::fs::remove_file(ready_path(s(&stale["token"])));
        }
    }
    ensure!(
        !pending_path().exists(),
        "Keep or revert the current preview first"
    );
    let panel = data().await?;
    let saved = read_settings()?;
    let request = if name == "apply" {
        serde_json::from_str(selection)?
    } else {
        ensure!(
            name == "preset" && ["desktop", "game", "toggle", "mirror"].contains(&selection),
            "Unknown display control"
        );
        let tv = options["tv"].as_str().unwrap_or("HDMI-A-1");
        let monitor = arr(&panel["monitors"])
            .iter()
            .find(|m| m["name"] == tv && truth(&m["connected"]))
            .context("Connect the TV before changing game mode")?;
        let enabled = if selection == "toggle" {
            !truth(&monitor["enabled"])
        } else {
            selection != "desktop"
        };
        let mut tv_settings = json!({"enabled":enabled});
        if !s(&monitor["settings"]["mirror"]).is_empty() {
            tv_settings["mirror"] = json!("");
        }
        let mut edits = vec![json!({"name":tv,"settings":tv_settings})];
        let copy = options["mirrorOutput"].as_str().unwrap_or("DP-2");
        for row in arr(&panel["monitors"])
            .iter()
            .filter(|r| truth(&r["connected"]) && r["name"] != tv)
        {
            if selection == "mirror" && row["name"] == copy {
                edits.push(json!({"name":copy,"settings":{"enabled":true,"mirror":tv}}));
            } else if row["settings"]["mirror"] == tv && selection != "mirror" {
                edits.push(json!({"name":row["name"],"settings":{"mirror":""}}));
            }
        }
        ensure!(
            selection != "mirror" || edits.iter().any(|e| e["name"] == copy),
            "Connect the mirror display first"
        );
        json!({"revision":panel["revision"],"edits":edits})
    };
    let (apply, before, after) = plan(&panel, &request, &saved)?;
    let token = format!("{}-{}", std::process::id(), (now() * 1_000_000.) as u64);
    let mut pending = json!({"token":token,"instance":instance(),"phase":"applying","deadline":now()+30.,"before":before,"apply":apply,"after":after,"error":""});
    // Atomic installation can leave this process on an unlinked inode. Use
    // the supported replacement path so systemd can also restart the guard
    // after the requesting frontend has exited.
    let running = std::env::current_exe()?;
    let raw = running.to_string_lossy();
    let executable = if running.is_file() {
        raw.into_owned()
    } else {
        let replacement = raw
            .strip_suffix(" (deleted)")
            .context("Display recovery executable is unavailable; reopen the bar")?;
        ensure!(
            PathBuf::from(replacement).is_file(),
            "Display recovery executable is unavailable; reopen the bar"
        );
        replacement.to_owned()
    };
    save(&pending_path(), &pending)?;
    let mut guard_command = vec![
        "systemd-run".into(),
        "--user".into(),
        "--collect".into(),
        "--quiet".into(),
        format!("--unit=bartjik-monitor-guard-{token}"),
        "--property=Restart=on-failure".into(),
        "--property=PartOf=graphical-session.target".into(),
    ];
    for key in [
        "XDG_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "HYPRLAND_INSTANCE_SIGNATURE",
        "WAYLAND_DISPLAY",
    ] {
        if let Ok(value) = std::env::var(key) {
            guard_command.push(format!("--setenv={key}={value}"));
        }
    }
    guard_command.extend([
        "--".into(),
        executable,
        "helper".into(),
        "monitors".into(),
        "guard".into(),
        token.clone(),
    ]);
    let start = command(&guard_command, 5., 4096, &[], None).await;
    if let Err(error) = start {
        std::fs::remove_file(pending_path())?;
        anyhow::bail!("Could not start display recovery; no changes were applied: {error}");
    }
    for _ in 0..50 {
        if ready_path(&token).exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if !ready_path(&token).exists() {
        std::fs::remove_file(pending_path())?;
        anyhow::bail!("Display recovery did not start; no changes were applied");
    }
    if let Err(e) = async {
        apply_rules(&apply).await?;
        wait_applied(&apply).await
    }
    .await
    {
        let restored = rollback(&pending).await;
        if restored.is_err() {
            pending["deadline"] = json!(now());
            save(&pending_path(), &pending)?;
        }
        return Err(e);
    }
    pending["phase"] = json!("confirm");
    pending["deadline"] = json!(now() + 20.);
    save(&pending_path(), &pending)?;
    Ok("Keep these display settings?".into())
}
pub fn demo() -> Value {
    let live = json!([
        {"name":"DP-1","model":"Samsung Odyssey G8","width":3440,"height":1440,"refreshRate":119.96,"x":0,"y":0,"scale":1,"transform":0,"disabled":false,"availableModes":["3440x1440@119.96Hz","3440x1440@60.00Hz"],"hardwareDetails":{"hdr":true,"vrrCapable":true},"colorManagementPreset":"srgb"},
        {"name":"DP-2","model":"HP 23f","width":1920,"height":1080,"refreshRate":74.97,"x":3440,"y":720,"scale":1,"transform":0,"disabled":false,"availableModes":["1920x1080@74.97Hz","1920x1080@60.00Hz"],"hardwareDetails":{"hdr":false,"vrrCapable":false},"colorManagementPreset":"srgb"},
        {"name":"HDMI-A-1","model":"LG TV","width":0,"height":0,"refreshRate":60.,"x":-1,"y":-1,"scale":1,"transform":0,"disabled":true,"availableModes":["3840x2160@60.00Hz","3840x2160@120.00Hz"],"hardwareDetails":{"hdr":true,"vrrCapable":true}}
    ]);
    let base = BTreeMap::from([
        ("DP-1".into(), default_settings()),
        ("DP-2".into(), default_settings()),
        (
            "HDMI-A-1".into(),
            merge(
                &default_settings(),
                &json!({"mode":"3840x2160@60.00","x":-3840,"y":-360,"enabled":false}),
            ),
        ),
    ]);
    let rows = catalogue(&live, &base, &json!({}), &json!({}));
    let mut row = item(
        "layout",
        None,
        Some("desktop_windows"),
        "symbol",
        json!({"active":false}),
        "Desktop Mode",
        json!({"leftClick":"monitor.toggle","rightClick":"monitor.panel","hoverEnter":"monitor.panel"}),
    );
    row["panel"] = json!({"title":"Displays","rows":[],"monitors":rows,"revision":revision(&rows),"pending":null,"tv":"HDMI-A-1","mirrorOutput":"DP-2"});
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(panel: &Value, edits: Value) -> Value {
        json!({"revision":panel["revision"],"edits":edits})
    }
    #[test]
    fn catalogue_retains_disconnected_outputs_and_scaled_rotations() {
        let source="hl.monitor({\noutput = \"DP-1\",\nmode = \"3440x1440@119.96\",\nposition = \"0x0\",\nscale = 1,\n})\nhl.monitor({\noutput = \"HDMI-A-1\",\nmode = \"3840x2160@60\",\nposition = \"auto-center-left\",\nscale = 1,\n})\nhl.monitor({\noutput = \"HDMI-A-1\",\ndisabled = true,\n})";
        let declared = configured(source);
        assert_eq!(declared["HDMI-A-1"]["x"], -3840);
        assert_eq!(declared["HDMI-A-1"]["y"], -360);
        let rows = catalogue(&json!([]), &declared, &json!({}), &json!({}));
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|r| r["connected"] == false && r["enabled"] == false));
        assert_eq!(
            logical(&json!({"mode":"3840x2160@60.00","scale":2,"transform":1})),
            (1080., 1920.)
        );
    }
    #[test]
    fn rejects_last_screen_disable_stale_edits_and_disconnected_changes() {
        let mut p = demo()["panel"].clone();
        p["monitors"][0]["configured"] = json!(false);
        assert!(plan(
            &p,
            &request(&p, json!([{"name":"DP-1","settings":{"x":200}}])),
            &json!({})
        )
        .unwrap_err()
        .to_string()
        .contains("explicit connector rule"));
        p["monitors"][0]["configured"] = json!(true);
        let all_off = json!([{"name":"DP-1","settings":{"enabled":false}},{"name":"DP-2","settings":{"enabled":false}}]);
        assert!(plan(&p, &request(&p, all_off), &json!({}))
            .unwrap_err()
            .to_string()
            .contains("at least one"));
        let mut req = request(&p, json!([{"name":"DP-2","settings":{"enabled":false}}]));
        req["revision"] = json!("old");
        assert!(plan(&p, &req, &json!({}))
            .unwrap_err()
            .to_string()
            .contains("changed"));
        p["monitors"][0]["connected"] = json!(false);
        assert!(plan(
            &p,
            &request(&p, json!([{"name":"DP-1","settings":{"enabled":true}}])),
            &json!({})
        )
        .unwrap_err()
        .to_string()
        .contains("disconnected"));
    }
    #[test]
    fn rejects_unadvertised_modes_invalid_scales_and_unsupported_hdr_vrr() {
        let p = demo()["panel"].clone();
        for settings in [
            json!({"mode":"9999x9999@500"}),
            json!({"scale":1.7}),
            json!({"transform":8}),
            json!({"x":40000}),
            json!({"vrr":2}),
            json!({"cm":"hdr"}),
            json!({"unknown":true}),
            json!({"enabled":"false"}),
        ] {
            assert!(plan(
                &p,
                &request(&p, json!([{"name":"DP-2","settings":settings}])),
                &json!({})
            )
            .is_err());
        }
        assert!(rule("DP-1\"); os.execute('bad')", &json!({"enabled":false})).is_err());
    }
    #[test]
    fn edits_preserve_other_settings_and_rollback_geometry() {
        let p = demo()["panel"].clone();
        let saved = json!({"DP-1":{"vrr":2},"HDMI-A-1":{"cm":"auto"}});
        let (apply,before,after)=plan(&p,&request(&p,json!([{"name":"DP-1","settings":{"x":100}},{"name":"DP-2","settings":{"enabled":false}}])),&saved).unwrap();
        assert_eq!(apply["DP-1"], json!({"x":100,"y":0}));
        assert_eq!(before["DP-1"], json!({"x":0,"y":0}));
        assert_eq!(before["DP-2"]["mode"], "1920x1080@74.97");
        assert_eq!(before["DP-2"]["enabled"], true);
        assert_eq!(after["DP-1"]["vrr"], 2);
        assert_eq!(after["HDMI-A-1"], saved["HDMI-A-1"]);
        let lua = rules_lua(&apply).unwrap();
        assert!(lua.find("DP-1").unwrap() < lua.find("DP-2").unwrap());
        assert!(!lua.contains("bitdepth"));
        let encoded = settings_lua(&after).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(
                encoded
                    .lines()
                    .next()
                    .unwrap()
                    .strip_prefix(PREFIX)
                    .unwrap()
            )
            .unwrap(),
            after
        );
    }
    #[test]
    fn enabling_tv_restores_its_configured_mode_and_position() {
        let p = demo()["panel"].clone();
        let (apply, before, _) = plan(
            &p,
            &request(&p, json!([{"name":"HDMI-A-1","settings":{"enabled":true}}])),
            &json!({}),
        )
        .unwrap();
        assert_eq!(apply["HDMI-A-1"]["mode"], "3840x2160@60.00");
        assert_eq!(apply["HDMI-A-1"]["x"], -3840);
        assert_eq!(before["HDMI-A-1"]["enabled"], false);
        assert!(applied(
            &json!([{"name":"HDMI-A-1","disabled":true}]),
            &before
        ));
    }
    #[test]
    fn mirroring_validates_sources_and_preserves_independent_placement() {
        let p = demo()["panel"].clone();
        for edits in [
            json!([{"name":"DP-2","settings":{"mirror":"DP-2"}}]),
            json!([{"name":"DP-2","settings":{"mirror":"HDMI-A-1"}}]),
            json!([{"name":"DP-2","settings":{"mirror":"missing"}}]),
            json!([{"name":"DP-2","settings":{"mirror":"DP-1"}},{"name":"DP-1","settings":{"mirror":"DP-2"}}]),
            json!([{"name":"HDMI-A-1","settings":{"enabled":true,"mirror":"DP-2"}},{"name":"DP-2","settings":{"mirror":"DP-1"}}]),
        ] {
            assert!(plan(&p, &request(&p, edits), &json!({})).is_err());
        }
        let (apply, before, after) = plan(
            &p,
            &request(
                &p,
                json!([
                    {"name":"HDMI-A-1","settings":{"enabled":true}},
                    {"name":"DP-2","settings":{"mirror":"HDMI-A-1"}}
                ]),
            ),
            &json!({}),
        )
        .unwrap();
        assert_eq!(apply["DP-2"], json!({"mirror":"HDMI-A-1","x":3440,"y":720}));
        assert_eq!(before["DP-2"], json!({"mirror":"","x":3440,"y":720}));
        assert_eq!(after["DP-2"]["mirror"], "HDMI-A-1");
        let live = json!([
            {"name":"DP-2","id":1,"mirrorOf":"2","width":1920,"height":1080,"refreshRate":60,"x":-3840,"y":-360,"scale":1,"transform":0,"disabled":false},
            {"name":"HDMI-A-1","id":2,"mirrorOf":"none","width":3840,"height":2160,"disabled":false}
        ]);
        let settings = live_settings(&live[0], &p["monitors"][1]["settings"], &live);
        assert_eq!(settings["mirror"], "HDMI-A-1");
        assert_eq!(settings["x"], 3440);
        assert!(applied(
            &live,
            &json!({"DP-2":{"mirror":"HDMI-A-1","x":3440,"y":720}})
        ));
        assert!(!applied(&live, &json!({"DP-2":{"mirror":""}})));
    }
}
