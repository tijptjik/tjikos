use crate::util::*;
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    os::unix::fs::PermissionsExt,
};
pub fn database() -> Result<Connection> {
    let dir = state();
    private_dir(&dir)?;
    let p = dir.join("history.sqlite3");
    let db = Connection::open(&p)?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    fs::set_permissions(p, fs::Permissions::from_mode(0o600))?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS events (id INTEGER PRIMARY KEY,time TEXT NOT NULL,event TEXT NOT NULL,notice_id TEXT,app TEXT,payload TEXT NOT NULL); CREATE INDEX IF NOT EXISTS events_time ON events(time);")?;
    let fresh = db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name='avatars'",
        [],
        |r| r.get::<_, i64>(0),
    )? == 0;
    db.execute_batch("CREATE TABLE IF NOT EXISTS avatars (chat_id TEXT PRIMARY KEY,title TEXT NOT NULL,image TEXT NOT NULL)")?;
    if fresh {
        let mut stmt = db.prepare("SELECT payload FROM events ORDER BY id")?;
        for s in stmt.query_map([], |r| r.get::<_, String>(0))? {
            let v: Value = serde_json::from_str(&s?)?;
            let _ = cache_avatar(&db, &v["notice"]);
        }
    }
    Ok(db)
}
fn images(db: &Connection) -> Result<HashSet<String>> {
    Ok(db
        .prepare("SELECT image FROM avatars")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?)
}
fn cache_avatar(db: &Connection, n: &Value) -> Result<()> {
    let d = &n["data"];
    let chat = d["chatId"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| d["chatId"].to_string());
    if n["type"] != "telegram"
        || truthy(&d["demo"])
        || !regex::Regex::new(r"^-?\d+$")?.is_match(&chat)
    {
        return Ok(());
    }
    let url = url::Url::parse(n["image"].as_str().unwrap_or(""))?;
    if url.scheme() != "file" || url.host_str().is_some() {
        return Ok(());
    }
    let path = url
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("Invalid photo URL"))?
        .canonicalize()?;
    let root = telegram_dir().canonicalize()?;
    if !["db/profile_photos", "files"]
        .iter()
        .any(|p| path.starts_with(root.join(p)))
    {
        return Ok(());
    }
    let meta = fs::metadata(&path)?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > 20 * 1024 * 1024 {
        return Ok(());
    }
    let content = fs::read(path)?;
    let dir = state().join("avatars");
    private_dir(&dir)?;
    let target = dir.join(format!("{:x}.jpg", Sha256::digest(&content)));
    if !target.exists() {
        private_write(&target, &content)?;
    }
    let old = images(db)?;
    db.execute(
        "INSERT OR REPLACE INTO avatars VALUES(?,?,?)",
        params![chat, n["title"].as_str().unwrap_or(""), uri(&target)?],
    )?;
    db.execute("DELETE FROM avatars WHERE rowid NOT IN (SELECT rowid FROM avatars ORDER BY rowid DESC LIMIT 512)",[])?;
    let retained = images(db)?;
    for image in old.difference(&retained) {
        if let Ok(p) = url::Url::parse(image).and_then(|u| {
            u.to_file_path()
                .map_err(|_| url::ParseError::RelativeUrlWithoutBase)
        }) {
            if p.parent() == Some(dir.as_path()) {
                let _ = fs::remove_file(p);
            }
        }
    }
    Ok(())
}
pub fn avatars(db: &Connection) -> Result<Value> {
    let mut chat = serde_json::Map::new();
    let mut title = serde_json::Map::new();
    let mut stmt = db.prepare("SELECT chat_id,title,image FROM avatars")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (c, t, i) = row?;
        if !url::Url::parse(&i)
            .ok()
            .and_then(|u| u.to_file_path().ok())
            .is_some_and(|p| p.is_file())
        {
            continue;
        }
        chat.insert(c, json!(i));
        if !t.is_empty() {
            let duplicate = title.contains_key(&t);
            title.insert(t, json!(if duplicate { "" } else { &i }));
        }
    }
    Ok(json!({"byChat":chat,"byTitle":title}))
}
pub fn prune(db: &Connection, h: &Value) -> Result<bool> {
    if h["enabled"] != true {
        return Ok(false);
    }
    let before = db.total_changes();
    let days = h["maxAgeDays"].as_f64().unwrap_or(0.);
    if days > 0. {
        let cutoff = chrono::Utc::now() - chrono::Duration::milliseconds((days * 86400000.) as i64);
        db.execute(
            "DELETE FROM events WHERE time < ?",
            [cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, false)],
        )?;
    }
    let max = h["maxEvents"].as_u64().unwrap_or(0);
    if max > 0 {
        db.execute("DELETE FROM events WHERE id <= COALESCE((SELECT id FROM events ORDER BY id DESC LIMIT 1 OFFSET ?),0)",[max])?;
    }
    Ok(before != db.total_changes())
}
pub fn record(db: &mut Connection, cfg: &Value, event: &Value) -> Result<()> {
    let n = &event["notice"];
    let _ = cache_avatar(db, n);
    if cfg["history"]["enabled"] != true
        || (truthy(&n["transient"]) && cfg["history"]["logTransient"] != true)
    {
        return Ok(());
    }
    let mut e = event.clone();
    if let Some(actions) = e["notice"]["actions"].as_array_mut() {
        for a in actions {
            if let Some(o) = a.as_object_mut() {
                o.remove("command");
            }
        }
    }
    e["time"] = json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, false));
    let tx = db.transaction()?;
    tx.execute(
        "INSERT INTO events(time,event,notice_id,app,payload) VALUES(?,?,?,?,?)",
        params![
            e["time"].as_str(),
            e["event"].as_str().context("Missing event name")?,
            n["id"].as_str(),
            n["app"].as_str().unwrap_or(""),
            e.to_string()
        ],
    )?;
    prune(&tx, &cfg["history"])?;
    tx.commit()?;
    Ok(())
}
pub fn notices(db: &Connection, limit: usize) -> Result<Value> {
    let mut seen = HashSet::new();
    let mut result = vec![];
    let mut stmt = db.prepare("SELECT payload FROM events ORDER BY id DESC")?;
    for raw in stmt.query_map([], |r| r.get::<_, String>(0))? {
        let e: Value = serde_json::from_str(&raw?)?;
        let mut n = e["notice"].clone();
        let Some(id) = n["id"].as_str().filter(|s| !s.is_empty()) else {
            continue;
        };
        let session = e["session"].as_str().unwrap_or("legacy");
        let key = format!("{session}/{id}");
        if !seen.insert(key.clone()) {
            continue;
        }
        n["historyKey"] = json!(key);
        n["session"] = json!(session);
        n["lastEvent"] = e["event"].clone();
        n["loggedAt"] = e["time"].clone();
        n["actions"] = json!([]);
        if n["image"].as_str().is_some_and(|s| s.starts_with("image:")) {
            n["image"] = json!("");
        }
        result.push(n);
    }
    result.sort_by(|a, b| {
        b["received"]
            .as_f64()
            .unwrap_or(0.)
            .total_cmp(&a["received"].as_f64().unwrap_or(0.))
    });
    if limit > 0 {
        result.truncate(limit)
    }
    Ok(json!(result))
}
pub fn paused() -> Result<bool> {
    let p = state().join("preferences.json");
    if !p.exists() {
        return Ok(false);
    }
    read_json(&p)?["paused"]
        .as_bool()
        .context("Invalid notification pause preference")
}
pub fn save_paused(v: &Value) -> Result<()> {
    let b = v
        .as_bool()
        .context("Notification pause preference must be boolean")?;
    private_write(
        &state().join("preferences.json"),
        json!({"paused":b}).to_string().as_bytes(),
    )
}
pub fn logs(
    limit: usize,
    follow: bool,
    json_output: bool,
    filters: HashMap<String, String>,
) -> Result<()> {
    let db = database()?;
    let mut last = 0i64;
    let mut first = true;
    loop {
        let mut clauses = vec!["id > ?".to_string()];
        let mut values: Vec<rusqlite::types::Value> = vec![last.into()];
        for (col, key) in [("app", "app"), ("event", "event"), ("time", "since")] {
            if let Some(v) = filters.get(key) {
                clauses.push(format!(
                    "{col} {} ?",
                    if key == "since" { ">=" } else { "=" }
                ));
                values.push(v.clone().into());
            }
        }
        values.push((limit as i64).into());
        let sql = format!(
            "SELECT id,payload FROM events WHERE {} ORDER BY id {} LIMIT ?",
            clauses.join(" AND "),
            if first { "DESC" } else { "ASC" }
        );
        let mut stmt = db.prepare(&sql)?;
        let mut rows = stmt
            .query_map(rusqlite::params_from_iter(values), |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if first {
            rows.reverse()
        }
        for (id, raw) in rows {
            last = last.max(id);
            let v: Value = serde_json::from_str(&raw)?;
            if json_output {
                emit(&v)
            } else {
                let clean = |s: &str| {
                    s.chars()
                        .map(|c| if c.is_control() { ' ' } else { c })
                        .collect::<String>()
                };
                let n = &v["notice"];
                println!(
                    "{}  {:10} {:14} {}: {}",
                    clean(v["time"].as_str().unwrap_or("")),
                    clean(v["event"].as_str().unwrap_or("")),
                    clean(n["id"].as_str().unwrap_or("")),
                    clean(n["app"].as_str().unwrap_or("")),
                    clean(n["title"].as_str().unwrap_or(""))
                );
                if let Some(body) = n["body"].as_str().filter(|s| !s.is_empty()) {
                    println!("  {}", clean(body));
                }
            }
        }
        first = false;
        if !follow {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    Ok(())
}
