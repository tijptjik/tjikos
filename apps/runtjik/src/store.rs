use crate::util::*;
use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{collections::HashMap, fs, os::unix::fs::PermissionsExt};
pub struct Store(pub Connection);
impl Store {
    pub fn open() -> Result<Self> {
        let path = xdg("state").join("history.sqlite3");
        private_dir(path.parent().unwrap())?;
        let c = Connection::open(&path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        c.busy_timeout(std::time::Duration::from_secs(5))?;
        c.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY,time REAL NOT NULL,entry_id TEXT NOT NULL,action TEXT NOT NULL,success INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS counts(key TEXT PRIMARY KEY,value INTEGER NOT NULL,last REAL NOT NULL); CREATE TABLE IF NOT EXISTS command_queries(seq INTEGER PRIMARY KEY AUTOINCREMENT,query TEXT NOT NULL UNIQUE); CREATE TABLE IF NOT EXISTS close_requests(entry_id TEXT NOT NULL,pid INTEGER NOT NULL,identity TEXT NOT NULL,stage INTEGER NOT NULL,PRIMARY KEY(entry_id,pid));")?;
        c.execute_batch("BEGIN IMMEDIATE")?;
        let exists: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='frecency')",
            [],
            |r| r.get(0),
        )?;
        let store = Self(c);
        if !exists {
            store.0.execute_batch("CREATE TABLE frecency(entry_id TEXT PRIMARY KEY,score REAL NOT NULL,last REAL NOT NULL)")?;
            store.rebuild_scores()?;
        }
        store.0.execute_batch("COMMIT")?;
        Ok(store)
    }
    fn add(&self, id: &str, time: f64) -> Result<()> {
        let old = self
            .0
            .query_row(
                "SELECT score,last FROM frecency WHERE entry_id=?",
                [id],
                |r| Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?)),
            )
            .ok();
        let reference = old.map(|(_, last)| last.max(time)).unwrap_or(time);
        let score =
            old.map(|(s, t)| decay(s, reference - t)).unwrap_or(0.) + decay(1., reference - time);
        self.0.execute(
            "INSERT OR REPLACE INTO frecency VALUES(?,?,?)",
            params![id, score, reference],
        )?;
        Ok(())
    }
    fn rebuild_scores(&self) -> Result<()> {
        self.0.execute("DELETE FROM frecency", [])?;
        let mut q=self.0.prepare("SELECT entry_id,time FROM events WHERE success=1 AND action IN ('launch','switch') ORDER BY time,seq")?;
        for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))? {
            let (id, t) = row?;
            self.add(&id, t)?;
        }
        Ok(())
    }
    pub fn rebuild(&self) -> Result<()> {
        self.0.execute_batch("BEGIN IMMEDIATE; DELETE FROM counts; INSERT INTO counts SELECT action || ':' || entry_id,COUNT(*),MAX(time) FROM events WHERE success=1 GROUP BY action,entry_id;")?;
        self.rebuild_scores()?;
        self.0.execute_batch("COMMIT")?;
        Ok(())
    }
    pub fn scores(&self) -> Result<HashMap<String, f64>> {
        let mut q = self.0.prepare("SELECT entry_id,score,last FROM frecency")?;
        let t = now();
        let rows = q
            .query_map([], |r| {
                Ok((r.get(0)?, decay(r.get(1)?, t - r.get::<_, f64>(2)?)))
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }
    pub fn counts(&self) -> Result<Value> {
        let mut q = self.0.prepare("SELECT key,value,last FROM counts")?;
        let mut v = json!({});
        for row in q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })? {
            let (k, n, t) = row?;
            v[k] = json!({"count":n,"last":t});
        }
        Ok(v)
    }
    pub fn history(&self) -> Result<Vec<String>> {
        let mut q = self
            .0
            .prepare("SELECT query FROM command_queries ORDER BY seq")?;
        let v = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(v)
    }
    pub fn remember(&self, q: &str) -> Result<()> {
        if q.trim().is_empty() || !q.starts_with(['!', '/', '?']) {
            return Ok(());
        }
        let tx = self.0.unchecked_transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO command_queries(query) VALUES(?)",
            [q],
        )?;
        tx.execute("DELETE FROM command_queries WHERE seq NOT IN(SELECT seq FROM command_queries ORDER BY seq DESC LIMIT 100)",[])?;
        tx.commit()?;
        Ok(())
    }
    pub fn record(&self, id: &str, action: &str, success: bool) -> Result<()> {
        let t = now();
        let tx = self.0.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO events(time,entry_id,action,success) VALUES(?,?,?,?)",
            params![t, id, action, success],
        )?;
        if success {
            tx.execute("INSERT INTO counts VALUES(?,1,?) ON CONFLICT(key) DO UPDATE SET value=value+1,last=excluded.last",params![format!("{action}:{id}"),t])?;
            if action == "launch" || action == "switch" {
                self.add(id, t)?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}
fn decay(score: f64, elapsed: f64) -> f64 {
    score * 2f64.powf(-elapsed.max(0.) / 604800.)
}
