use crate::util::*;
use serde_json::{json, Value};
pub fn items(provider: &str, o: &Value) -> Vec<Value> {
    if let Some(data) = fixture() {
        if let Some(rows) = fixture_items(provider, &data) {
            return rows;
        }
    }
    let fixtures: Value = serde_json::from_str(
        &include_str!("demo.json").replace("@ROOT@", &root().to_string_lossy()),
    )
    .unwrap();
    let mut rows = arr(&fixtures[provider]).to_vec();
    if provider == "battery" {
        let mut row = crate::battery::row(
            o["demoPercent"].as_f64().unwrap_or(67.),
            truth(&o["demoExternal"]),
        );
        row["panel"] = crate::battery::panel_data(
            &row["battery"],
            &json!({"State":if truth(&row["battery"]["external"]){1}else{2},"TimeToFull":2520,"TimeToEmpty":14820}),
            &json!({"ActiveProfile":o["demoProfile"].as_str().unwrap_or("balanced"),"Profiles":crate::battery::PROFILES.iter().map(|(k,_)|json!({"Profile":{"data":k}})).collect::<Vec<_>>()}),
        );
        return vec![row];
    }
    if provider == "workspaces" {
        if truth(&o["demoPresentation"]) {
            rows.retain(|r| r["id"] != "14");
        }
        for r in &mut rows {
            let id = st(&r["id"]);
            if truth(&o["labels"][&id]) {
                r["text"]["value"] = o["labels"][&id].clone();
            }
            if ["circles", "circles_name"].contains(&s(&o["label"])) {
                r["type"] = json!(if o["label"] == "circles" {
                    "icon"
                } else {
                    "icon_text"
                });
                r["icon"] = json!({"kind":"image","value":icon(if id=="3"{"circle-filled"}else{"circle-open"}),"style":{},"actions":{}});
                if o["label"] == "circles" {
                    r.as_object_mut().unwrap().remove("text");
                }
            }
        }
    }
    if provider == "datetime" {
        rows[0]["panel"] = crate::details::world_clock(&o["worldClocks"], Some(1781524800));
    }
    if provider == "codex" {
        for (r, h) in rows[0]["panel"]["rows"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .filter(|r| r["group"] == "account")
            .zip([84, 56, 121, 102])
        {
            r["resetAt"] = json!(now() + h as f64 * 3600.);
        }
    }
    if provider == "icontray" {
        if o["demoTrayFixtures"].is_array() {
            return arr(&o["demoTrayFixtures"])
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let override_ = s(&o["iconOverrides"][s(&f[0])]);
                    let path = if let Some(n) = override_.strip_prefix("builtin:") {
                        icon(&format!("tray/{n}"))
                    } else {
                        st(&f[2])
                    };
                    item(
                        &format!("tray-{i}"),
                        None,
                        Some(&path),
                        "image",
                        json!({}),
                        s(&f[1]),
                        json!({}),
                    )
                })
                .collect();
        }
        for (r, (name, id, title)) in rows.iter_mut().zip([
            ("chatgpt", "ChatGPT", "ChatGPT"),
            ("telegram", "TelegramDesktop", "Telegram"),
            ("whatsapp", "ZapZap", "WhatsApp"),
        ]) {
            if o["iconOverrides"][id] == format!("builtin:{name}-outline") {
                r["icon"]["value"] = json!(icon(&format!("tray/{name}-outline")));
            }
            if truth(&o["demoPresentation"]) {
                r["tooltip"] = json!(title);
            }
        }
    }
    if truth(&o["demoPresentation"]) {
        if provider == "playerctl" {
            let r = &mut rows[0];
            r["artist"]["value"] = json!("Karla Blum");
            r["track"]["value"] = json!("Acid Rain");
            r["text"]["value"] = json!("Acid Rain");
            r["media"]["artist"] = json!("Karla Blum");
            r["media"]["title"] = json!("Acid Rain");
            r["media"]["album"] = json!("Acid Rain");
        } else if let Some(r) = rows.first_mut() {
            let tip = match provider {
                "DND" => Some("Notifications : ON"),
                "caffeine" => Some("Idle Inhibit : ON"),
                "nightlight" => Some("Nightshift : OFF"),
                "monitor-layout" => Some("Desktop Mode"),
                "audio" => Some("Volume: 42%"),
                "locale" => Some("English"),
                _ => None,
            };
            if let Some(t) = tip {
                r["tooltip"] = json!(t);
            }
            if provider == "nightlight" {
                r["icon"]["value"] = json!("light_mode");
            }
        }
    }
    rows
}
static COUNTRY: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static TAILNET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
pub fn fixture() -> Option<Value> {
    let p = std::env::var_os("BARTJIK_DEMO_FIXTURE")?;
    let p = std::path::PathBuf::from(p);
    if p.metadata().ok()?.len() > 4_000_000 {
        return None;
    }
    let v = read(p).ok()?;
    if v["synthetic"] != true {
        return None;
    }
    Some(v)
}
pub fn control(c: &Value) -> bool {
    if fixture().is_none() {
        return false;
    }
    if c["module"] == "vpn"
        && c["action"] == "country"
        && ["jp", "nl", "sg"].contains(&s(&c["selection"]))
    {
        *COUNTRY.lock().unwrap() = Some(st(&c["selection"]));
        return true;
    }
    if c["module"] == "tailnet" && c["action"] == "toggle" {
        TAILNET.fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
        return true;
    }
    false
}
fn fixture_items(provider: &str, data: &Value) -> Option<Vec<Value>> {
    let rows = data["providers"].get(provider)?.as_array()?.clone();
    let mut rows = rows;
    if provider == "mullvad" {
        if let Some(country) = COUNTRY.lock().unwrap().as_deref() {
            let location = match country {
                "nl" => "Netherlands, Amsterdam",
                "sg" => "Singapore, Singapore",
                _ => "Japan, Tokyo",
            };
            let cat = json!({"jp":{"id":"jp","name":"Japan","cities":[{"id":"tyo","name":"Tokyo"}]},"nl":{"id":"nl","name":"Netherlands","cities":[{"id":"ams","name":"Amsterdam"}]},"sg":{"id":"sg","name":"Singapore","cities":[{"id":"sin","name":"Singapore"}]}});
            if let Some(r) = rows.first_mut() {
                r["icon"]["value"] = json!(icon(&format!("flags/{country}")));
                r["panel"] = crate::connectivity::mullvad_panel(
                    "connected",
                    location,
                    &cat,
                    &json!(["jp", "nl", "sg"]),
                );
            }
        }
    }
    if provider == "tailscale" {
        if let Some(r) = rows.first_mut() {
            r["panel"]["connection"] =
                json!(if TAILNET.load(std::sync::atomic::Ordering::Relaxed) {
                    "Running"
                } else {
                    "Stopped"
                });
        }
    }
    Some(rows)
}
