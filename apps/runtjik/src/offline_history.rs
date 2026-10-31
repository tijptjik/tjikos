//! Explicitly configured, read-only Places access when no bridge is connected.
//! No copied database, persistent index, profile discovery, or history writes.
use crate::{config, discovery::Entry, util::*};
use anyhow::{bail, ensure, Context, Result};
use rusqlite::{params, Connection, OpenFlags};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

fn open(profile: &Path) -> Result<Connection> {
    ensure!(
        profile.is_absolute(),
        "History profile must be an absolute path"
    );
    let db = Connection::open_with_flags(
        profile.join("places.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("Cannot read configured browser history; check the profile path")?;
    db.busy_timeout(Duration::from_millis(250))?;
    Ok(db)
}

fn search(profile: &Path, index: usize, text: &str) -> Result<Vec<Entry>> {
    let db = open(profile)?;
    let mut statement = db.prepare(
        "SELECT guid, url, coalesce(nullif(title, ''), url), visit_count, last_visit_date
         FROM moz_places WHERE visit_count > 0 AND hidden = 0 AND last_visit_date IS NOT NULL
         AND (url LIKE 'https://%' OR url LIKE 'http://%')
         AND (?1 = '' OR instr(lower(coalesce(title, '') || ' ' || url), lower(?1)) > 0)
         ORDER BY last_visit_date DESC LIMIT 100",
    )?;
    let rows = statement.query_map([text], |row| {
        let guid: String = row.get(0)?;
        let url: String = row.get(1)?;
        let count: i64 = row.get(3)?;
        let visited: i64 = row.get(4)?;
        Ok(Entry {
            id: format!("Browser:offline:{index}:{guid}"),
            name: row.get(2)?,
            subtitle: format!("{url} · {count} visits · offline"),
            icon: "document-open-recent".into(),
            provider: "Browser".into(),
            keywords: String::new(),
            data: json!({"history":true,"offline_history":true,"profile_index":index,
                "guid":guid,"url":url,"lastVisitTime":visited as f64 / 1000.0}),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn query(text: &str) -> Result<Option<Vec<Entry>>> {
    let settings = config::load()?;
    let profiles = strings(&settings["plugin_settings"]["Browser"]["history_profiles"]);
    if profiles.is_empty() {
        return Ok(None);
    }
    let mut rows = vec![];
    for (index, profile) in profiles.iter().enumerate() {
        rows.extend(search(Path::new(profile), index, text)?);
    }
    rows.sort_by(|a, b| {
        b.data["lastVisitTime"]
            .as_f64()
            .unwrap_or_default()
            .total_cmp(&a.data["lastVisitTime"].as_f64().unwrap_or_default())
    });
    rows.truncate(100);
    Ok(Some(rows))
}

fn launch_args(settings: &Value, entry: &Entry) -> Result<Vec<String>> {
    let index = entry.data["profile_index"]
        .as_u64()
        .context("Invalid history profile")? as usize;
    let profile = settings["history_profiles"]
        .get(index)
        .and_then(Value::as_str)
        .context("History configuration changed; refresh the search")?;
    let url = entry.data["url"].as_str().context("Missing history URL")?;
    ensure!(
        ["http", "https"].contains(&url::Url::parse(url)?.scheme()),
        "Only HTTP(S) history can be opened"
    );
    let db = open(Path::new(profile))?;
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM moz_places WHERE guid = ?1 AND url = ?2
         AND visit_count > 0 AND hidden = 0 AND last_visit_date IS NOT NULL)",
        params![s(&entry.data["guid"]), url],
        |row| row.get(0),
    )?;
    ensure!(exists, "History entry was removed; refresh the search");
    let mut args = strings(&settings["history_browser"]);
    ensure!(!args.is_empty(), "Configure Browser.history_browser");
    args.extend([
        "--profile".into(),
        profile.into(),
        "--new-tab".into(),
        url.into(),
    ]);
    Ok(args)
}

pub fn activate(entry: &Entry, mode: &str) -> Result<Value> {
    if mode == "close" {
        bail!("Offline history is read-only. Start the browser with the Runtjik extension, refresh /history, then delete the entry.");
    }
    if mode == "switch" {
        return Ok(
            json!({"kind":"message","message":"Enter opens this item in its browser profile. Offline history is read-only."}),
        );
    }
    let settings = config::load()?;
    let args = launch_args(&settings["plugin_settings"]["Browser"], entry)?;
    dispatch(args, &settings)?;
    Ok(json!({"kind":"activated","demo":false}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let db = Connection::open(dir.path().join("places.sqlite")).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE moz_places(guid TEXT, url TEXT, title TEXT, visit_count INTEGER, last_visit_date INTEGER, hidden INTEGER);
            INSERT INTO moz_places VALUES ('one','https://example.org/a','First % page',3,1000000,0),
                ('two','http://example.org/b','Newest',2,2000000,0),
                ('file','file:///private','Secret',1,3000000,0),
                ('hidden','https://example.org/hidden','Hidden',1,4000000,1),
                ('unvisited','https://example.org/bookmark','Bookmark',0,NULL,0);").unwrap();
        (dir, db)
    }
    #[test]
    fn searches_read_only_wal_history_and_revalidates_before_opening() {
        let (dir, writer) = fixture();
        let rows = search(dir.path(), 0, "").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "Newest");
        assert_eq!(rows[0].data["lastVisitTime"], 2000.0);
        assert_eq!(search(dir.path(), 0, "%").unwrap().len(), 1);
        assert_eq!(search(dir.path(), 0, "FIRST").unwrap().len(), 1);
        let reader = open(dir.path()).unwrap();
        assert!(reader.execute("DELETE FROM moz_places", []).is_err());
        let settings = json!({"history_profiles":[dir.path()],"history_browser":["zen-browser"]});
        let args = launch_args(&settings, &rows[0]).unwrap();
        assert_eq!(args[1], "--profile");
        assert_eq!(args[2], dir.path().to_str().unwrap());
        assert_eq!(args[4], "http://example.org/b");
        assert!(activate(&rows[0], "close").is_err());
        writer
            .execute("DELETE FROM moz_places WHERE guid = 'two'", [])
            .unwrap();
        assert!(launch_args(&settings, &rows[0]).is_err());
        assert!(search(&dir.path().join("missing"), 0, "").is_err());
        assert!(!dir.path().join("missing").exists());
    }
}
