use crate::model;
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{os::unix::fs::OpenOptionsExt, path::Path, time::Duration};
use uuid::Uuid;

pub fn connect(path: &Path) -> Result<Connection> {
    let c = Connection::open(path)?;
    c.busy_timeout(Duration::from_secs(10))?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
        CREATE TABLE IF NOT EXISTS tasks(id TEXT PRIMARY KEY,body TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS requests(id TEXT PRIMARY KEY,digest TEXT NOT NULL,response TEXT NOT NULL,created_at TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS devices(id TEXT PRIMARY KEY,owner TEXT NOT NULL,name TEXT NOT NULL,notifications INTEGER NOT NULL DEFAULT 0,last_seen TEXT NOT NULL);
        PRAGMA user_version=1;")?;
    Ok(c)
}
pub fn tasks(c: &Connection) -> Result<Vec<Value>> {
    let mut s = c.prepare("SELECT body FROM tasks ORDER BY id")?;
    let rows = s.query_map([], |r| r.get::<_, String>(0))?;
    rows.map(|r| {
        let mut v: Value = serde_json::from_str(&r?)?;
        model::defaults(&mut v);
        Ok(v)
    })
    .collect()
}
pub fn request(path: &Path, r: Value, owner: &str, device: &str) -> Result<Value> {
    let mut c = connect(path)?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let op = r["action"].as_str().unwrap_or("list");
    if op == "capabilities" {
        return Ok(
            json!({"reading":true,"idempotent_create":true,"agent_tasks":true,"protocol":1,"sqlite":true}),
        );
    }
    if op == "list" {
        return Ok(json!(tasks(&tx)?));
    }
    ensure!(
        r.get("directory").is_none(),
        "directory cannot be selected remotely"
    );
    if ["claim", "ack", "release"].contains(&op) {
        let allowed = tx
            .query_row(
                "SELECT notifications FROM devices WHERE owner=? AND id=?",
                params![owner, device],
                |r| r.get::<_, bool>(0),
            )
            .unwrap_or(false);
        ensure!(allowed, "Enroll this device and enable notifications first");
    }
    let key = r["request_id"]
        .as_str()
        .context("Mutations need a UUID request_id")?;
    Uuid::parse_str(key)?;
    let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&r)?));
    let previous = tx.query_row(
        "SELECT digest,response FROM requests WHERE id=?",
        [key],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    );
    if let Ok((old, reply)) = previous {
        ensure!(
            old == digest,
            "Request ID was already used for different content"
        );
        return Ok(serde_json::from_str(&reply)?);
    }
    let result = model::mutate(&tasks(&tx)?, &r)?;
    if result.is_object() {
        tx.execute(
            "INSERT INTO tasks VALUES (?,?) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
            params![result["id"].as_str(), serde_json::to_string(&result)?],
        )?;
    }
    tx.execute(
        "INSERT INTO requests VALUES (?,?,?,?)",
        params![key, digest, serde_json::to_string(&result)?, model::now()],
    )?;
    tx.commit()?;
    Ok(result)
}
pub fn devices(path: &Path, owner: &str, update: Option<Value>) -> Result<Value> {
    let mut c = connect(path)?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(v) = update {
        let id = v["id"].as_str().context("Device ID required")?;
        Uuid::parse_str(id)?;
        let name = v["name"].as_str().context("Device name required")?;
        ensure!(!name.is_empty() && name.len() <= 100, "Invalid device name");
        let enabled = v["notifications"]
            .as_bool()
            .context("notifications must be a boolean")?;
        if let Ok(old) = tx.query_row("SELECT owner FROM devices WHERE id=?", [id], |r| {
            r.get::<_, String>(0)
        }) {
            ensure!(old == owner, "Device belongs to another identity");
        }
        tx.execute("INSERT INTO devices VALUES (?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,notifications=excluded.notifications,last_seen=excluded.last_seen",params![id,owner,name,enabled,model::now()])?;
    }
    let result = {
        let mut s = tx.prepare(
            "SELECT id,name,notifications,last_seen FROM devices WHERE owner=? ORDER BY name",
        )?;
        let rows=s.query_map([owner],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"notifications":r.get::<_,bool>(2)?,"last_seen":r.get::<_,String>(3)?})))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    tx.commit()?;
    Ok(json!({"devices":result}))
}
pub fn snapshot(path: &Path, dest: &Path) -> Result<()> {
    ensure!(path.is_file(), "Source database does not exist");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dest)?;
    let source = connect(path)?;
    let mut target = Connection::open(dest)?;
    rusqlite::backup::Backup::new(&source, &mut target)?.run_to_completion(
        128,
        Duration::from_millis(10),
        None,
    )?;
    let integrity: String = target.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    ensure!(integrity == "ok", "Snapshot integrity check failed");
    Ok(())
}
pub fn import(path: &Path, source: &Path) -> Result<usize> {
    let mut c = connect(path)?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut count = 0;
    for entry in std::fs::read_dir(source)? {
        let p = entry?.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let mut v: Value = serde_json::from_slice(&std::fs::read(&p)?)?;
        ensure!(v.is_object(), "Task must be an object");
        Uuid::parse_str(v["id"].as_str().context("Task ID required")?)?;
        model::defaults(&mut v);
        model::validate(&mut v)?;
        let id = v["id"].as_str().unwrap();
        if let Ok(existing) = tx.query_row("SELECT body FROM tasks WHERE id=?", [id], |r| {
            r.get::<_, String>(0)
        }) {
            ensure!(
                serde_json::from_str::<Value>(&existing)? == v,
                "Import conflicts with existing task {id}"
            );
        } else {
            tx.execute(
                "INSERT INTO tasks VALUES (?,?)",
                params![id, serde_json::to_string(&v)?],
            )?;
            count += 1;
        }
    }
    tx.commit()?;
    Ok(count)
}
