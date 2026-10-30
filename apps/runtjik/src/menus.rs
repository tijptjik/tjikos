use crate::{config, discovery::Entry, paths, settings, util::*};
use anyhow::Result;
use serde_json::{json, Value};
#[derive(Clone)]
pub struct Command {
    pub id: String,
    pub description: String,
    pub icon: String,
    pub aliases: Vec<String>,
    pub provider: String,
    pub app: bool,
}
pub fn commands(settings: &Value) -> Vec<Command> {
    let mut rows = vec![];
    for (id, description, icon) in [
        (
            "/processes",
            "Search applications with running windows",
            "runtjik-stats",
        ),
        (
            "/run",
            "Search previously used terminal commands",
            "org.gnome.Terminal",
        ),
        (
            "/refresh",
            "Rebuild the application and command index",
            "runtjik-refresh",
        ),
        (
            "/pin",
            "Toggle keeping the launcher open on outside clicks",
            "runtjik-pin",
        ),
        ("/grid", "Show applications as an icon grid", "runtjik-grid"),
        ("/list", "Show applications as a list", "runtjik-list"),
        (
            "/stats",
            "Show cumulative launches and window switches",
            "runtjik-stats",
        ),
        (
            "/config",
            "View and change featured settings",
            "runtjik-config",
        ),
        (
            "/bindings",
            "Show keyboard shortcuts and dragging",
            "runtjik-shortcuts",
        ),
    ] {
        rows.push(Command {
            id: id.into(),
            description: description.into(),
            icon: icon.into(),
            aliases: if id == "/bindings" {
                vec!["/shortcuts".into()]
            } else {
                vec![]
            },
            provider: String::new(),
            app: false,
        });
    }
    let enabled = strings(&settings["plugins"]["enabled"]);
    let mut seen = std::collections::HashSet::new();
    for name in enabled {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(p) = config::contract()["providers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
        else {
            continue;
        };
        for c in p["commands"].as_array().unwrap() {
            let id = s(&c[0]);
            rows.push(Command {
                id: id.into(),
                description: s(&c[1]).into(),
                icon: s(&p["icon"]).into(),
                aliases: p["aliases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|a| a[1] == id)
                    .map(|a| s(&a[0]).to_owned())
                    .collect(),
                provider: name.clone(),
                app: p["mode"] == "app",
            });
        }
    }
    for c in &mut rows {
        let name = format!(
            "runtjik-{}",
            if c.id == "/bindings" {
                "shortcuts"
            } else {
                &c.id[1..]
            }
        );
        let path = paths::root()
            .join("qml/icons/commands")
            .join(format!("{name}.svg"));
        if path.is_file() {
            c.icon = path.to_string_lossy().into();
        }
    }
    rows
}
pub fn entries(settings: &Value) -> Vec<Entry> {
    commands(settings)
        .into_iter()
        .map(|c| Entry {
            id: c.id.clone(),
            name: c.id,
            subtitle: c.description,
            icon: c.icon,
            provider: "internal".into(),
            keywords: c.aliases.join(" "),
            data: json!({}),
        })
        .collect()
}
fn control_icon(name: &str) -> String {
    let path = paths::root()
        .join("qml/icons/commands")
        .join(format!("{name}.svg"));
    if path.is_file() {
        path.to_string_lossy().into()
    } else {
        name.into()
    }
}
pub fn query(q: &str, settings: &Value) -> Result<Option<Vec<Value>>> {
    let rows = if q.trim() == "/bindings" || q.trim() == "/shortcuts" {
        config::contract()["shortcuts"].as_array().unwrap().iter().enumerate().map(|(i,r)|json!({"id":format!("/shortcuts/{i}"),"name":r[0],"subtitle":r[1],"provider":"internal","icon":control_icon("runtjik-shortcuts"),"count":0,"windows":0,"disabled":true})).collect()
    } else if q == "/config" || q.starts_with("/config ") {
        let mut parts = q.splitn(3, char::is_whitespace).filter(|s| !s.is_empty());
        parts.next();
        let key = parts.next();
        let text = parts.next();
        let mut rows = vec![];
        if let (Some(key), Some(text)) = (key, text) {
            let parsed = settings::value(key, text).and_then(|v| {
                let source = std::fs::read_to_string(config::path()).unwrap_or_default();
                settings::edit(&source, key, &v)?;
                Ok(v)
            });
            match parsed{Ok(value)=>rows.push(json!({"id":q,"name":format!("Set {key} = {}",settings::display(&value)),"subtitle":"Enter saves runtjik.conf and applies this setting","disabled":false})),Err(e)=>rows.push(json!({"id":"/config/error","name":"Cannot save this value","subtitle":e.to_string(),"disabled":true}))}
        } else {
            for (k, description) in settings::featured(settings) {
                if key.is_some_and(|key| !k.starts_with(key)) {
                    continue;
                }
                rows.push(json!({"id":format!("/config {k}"),"name":format!("{k} = {}",settings::display(settings::get(settings,&k))),"subtitle":description,"disabled":false}));
            }
        }
        for row in &mut rows {
            row["provider"] = json!("internal");
            row["icon"] = json!(control_icon("runtjik-config"));
            row["count"] = json!(0);
            row["windows"] = json!(0);
        }
        rows
    } else if q.starts_with('/') {
        let commands = commands(settings);
        if commands.iter().any(|c| {
            !c.provider.is_empty()
                && (q.split_whitespace().next() == Some(c.id.as_str())
                    || c.aliases
                        .iter()
                        .any(|a| q.split_whitespace().next() == Some(a)))
        }) {
            return Ok(None);
        }
        let mut commands = commands;
        commands.sort_by(|a, b| a.id.cmp(&b.id));
        commands.into_iter().filter(|c|c.id.starts_with(q)||c.aliases.iter().any(|a|a.starts_with(q))).map(|c|{let suffix=if c.id=="/processes"{"  ·  ?".into()}else if c.id=="/run"{"  ·  !!".into()}else if c.aliases.is_empty(){String::new()}else{format!("  ·  {}",c.aliases.join(", "))};json!({"id":c.id,"name":format!("{}{suffix}",c.id),"subtitle":c.description,"icon":c.icon,"provider":"internal","count":0,"windows":0})}).collect()
    } else {
        return Ok(None);
    };
    Ok(Some(rows))
}
