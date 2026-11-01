use crate::model;
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{os::unix::fs::OpenOptionsExt, path::Path, time::Duration};
use tasktjik_api::images;
use uuid::Uuid;

pub fn connect(path: &Path) -> Result<Connection> {
    let c = Connection::open(path)?;
    c.busy_timeout(Duration::from_secs(10))?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
        CREATE TABLE IF NOT EXISTS tasks(id TEXT PRIMARY KEY,body TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS images(id TEXT PRIMARY KEY,bytes BLOB NOT NULL);
        CREATE TABLE IF NOT EXISTS requests(id TEXT PRIMARY KEY,digest TEXT NOT NULL,response TEXT NOT NULL,created_at TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS devices(id TEXT PRIMARY KEY,owner TEXT NOT NULL,name TEXT NOT NULL,notifications INTEGER NOT NULL DEFAULT 0,last_seen TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS undo_history(seq INTEGER PRIMARY KEY AUTOINCREMENT,owner TEXT NOT NULL,device TEXT NOT NULL,task_id TEXT NOT NULL,before_body TEXT NOT NULL,after_body TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS undo_history_device ON undo_history(owner,device,seq);
        PRAGMA user_version=3;")?;
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
            json!({"reading":true,"idempotent_create":true,"agent_tasks":true,"images":true,"protocol":1,"sqlite":true,"undo":true}),
        );
    }
    if op == "list" {
        return Ok(json!(tasks(&tx)?));
    }
    if op == "image-get" {
        let tasks = tasks(&tx)?;
        let metadata = images::attached(&tasks, &r)?;
        let bytes: Vec<u8> = tx.query_row(
            "SELECT bytes FROM images WHERE id=?",
            [metadata["id"].as_str()],
            |row| row.get(0),
        )?;
        return Ok(images::source(metadata, &bytes));
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
    let before_tasks = tasks(&tx)?;
    let result = if op == "undo" {
        let (seq, id, before, after): (i64, String, String, String) = tx.query_row(
            "SELECT seq,task_id,before_body,after_body FROM undo_history WHERE owner=? AND device=? ORDER BY seq DESC LIMIT 1",
            params![owner, device], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
        ).context("Nothing to undo on this device")?;
        ensure!(
            r.get("id").is_none() || r["id"] == id,
            "Undo target changed; the latest action belongs to another task"
        );
        let mut previous: Value = serde_json::from_str(&before)?;
        let mut expected: Value = serde_json::from_str(&after)?;
        // Undo entries written before attachments existed have no images field.
        // Compare and restore them using the same defaults as current task rows.
        if previous.is_object() {
            model::defaults(&mut previous);
        }
        if expected.is_object() {
            model::defaults(&mut expected);
        }
        ensure!(
            r["undo_action"] != "purge" || expected.is_null(),
            "Undo target changed; the latest action is not a purge"
        );
        let current = before_tasks
            .iter()
            .find(|t| t["id"] == id)
            .cloned()
            .unwrap_or(Value::Null);
        ensure!(
            current == expected,
            "Task changed since this action; undo would overwrite newer changes"
        );
        let restored = if previous.is_null() {
            json!({"id":id,"deleted":true})
        } else {
            previous["revision"] = json!(current["revision"]
                .as_u64()
                .unwrap_or(previous["revision"].as_u64().unwrap_or(0))
                .checked_add(1)
                .context("Invalid revision")?);
            previous["updatedAt"] = json!(model::now());
            if current["events"].is_array() {
                previous["events"] = current["events"].clone();
            }
            previous["events"]
                .as_array_mut()
                .context("Invalid event history")?
                .push(json!({"action":"undo","actor":owner,"at":model::now()}));
            model::validate(&mut previous)?;
            previous
        };
        tx.execute("DELETE FROM undo_history WHERE seq=?", [seq])?;
        // Keep consecutive undos of this task possible despite the new revision.
        // Only the exact previous snapshot is advanced; unrelated edits stay protected.
        if restored["deleted"] != true {
            tx.execute("UPDATE undo_history SET after_body=? WHERE owner=? AND device=? AND task_id=? AND after_body=?",
                params![serde_json::to_string(&restored)?,owner,device,id,before])?;
        }
        restored
    } else {
        let result = model::mutate(&before_tasks, &r)?;
        if op == "attach-image" {
            let (metadata, bytes) =
                images::decode(r["data"].as_str().context("Image data required")?)?;
            tx.execute(
                "INSERT OR IGNORE INTO images VALUES (?,?)",
                params![metadata["id"].as_str(), bytes],
            )?;
        }
        if [
            "create",
            "edit",
            "toggle",
            "done",
            "status",
            "snooze",
            "verify",
            "purge",
            "move",
            "attach-image",
        ]
        .contains(&op)
            && result["id"].is_string()
        {
            let before = before_tasks
                .iter()
                .find(|t| t["id"] == result["id"])
                .cloned()
                .unwrap_or(Value::Null);
            let after = if result["deleted"] == true {
                Value::Null
            } else {
                result.clone()
            };
            tx.execute("INSERT INTO undo_history(owner,device,task_id,before_body,after_body) VALUES (?,?,?,?,?)",
                params![owner,device,result["id"].as_str(),serde_json::to_string(&before)?,serde_json::to_string(&after)?])?;
            tx.execute("DELETE FROM undo_history WHERE owner=? AND device=? AND seq NOT IN (SELECT seq FROM undo_history WHERE owner=? AND device=? ORDER BY seq DESC LIMIT 100)",params![owner,device,owner,device])?;
        }
        result
    };
    if result["deleted"] == true {
        tx.execute("DELETE FROM tasks WHERE id=?", [result["id"].as_str()])?;
    } else if result.is_object() {
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
        for metadata in v["images"].as_array().context("Invalid task images")? {
            let id = metadata["id"].as_str().context("Invalid image ID")?;
            let bytes =
                std::fs::read(source.join("images").join(id)).context("Missing imported image")?;
            ensure!(
                images::inspect(&bytes)? == *metadata,
                "Imported image does not match task metadata"
            );
            tx.execute(
                "INSERT OR IGNORE INTO images VALUES (?,?)",
                params![id, bytes],
            )?;
        }
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

#[cfg(test)]
mod purge_tests {
    use super::*;

    #[test]
    fn purge_checks_revision_and_retries_without_recreating_task() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.db");
        let created = request(&path, json!({"action":"create","request_id":Uuid::new_v4().to_string(),"fields":{"title":"Synthetic purge"}}), "test", "test").unwrap();
        let mut purge = json!({"action":"purge","request_id":Uuid::new_v4().to_string(),"id":created["id"],"revision":0});
        assert!(request(&path, purge.clone(), "test", "test").is_err());
        assert_eq!(tasks(&connect(&path).unwrap()).unwrap().len(), 1);
        purge["revision"] = created["revision"].clone();
        let result = request(&path, purge.clone(), "test", "test").unwrap();
        assert_eq!(result["deleted"], true);
        assert_eq!(request(&path, purge, "test", "test").unwrap(), result);
        assert!(tasks(&connect(&path).unwrap()).unwrap().is_empty());
    }
}

#[cfg(test)]
mod undo_tests {
    use super::*;
    fn call(path: &Path, device: &str, mut value: Value) -> Result<Value> {
        value["request_id"] = json!(Uuid::new_v4().to_string());
        value["actor"] = json!("owner");
        request(path, value, "owner", device)
    }
    #[test]
    fn undo_is_sequential_idempotent_and_device_scoped() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("undo.sqlite3");
        let first = call(
            &path,
            "fi",
            json!({"action":"create","fields":{"title":"Original","tags":["keep"]}}),
        )
        .unwrap();
        assert!(call(&path, "li", json!({"action":"undo"})).is_err());
        assert!(call(&path, "fi", json!({"action":"undo","id":"wrong-task"})).is_err());
        assert!(call(
            &path,
            "fi",
            json!({"action":"undo","id":first["id"],"undo_action":"purge"})
        )
        .is_err());
        let edit = call(&path,"fi",json!({"action":"edit","id":first["id"],"revision":first["revision"],"fields":{"title":"Changed"}})).unwrap();
        call(
            &path,
            "fi",
            json!({"action":"toggle","id":edit["id"],"revision":edit["revision"]}),
        )
        .unwrap();
        let undone = call(&path, "fi", json!({"action":"undo"})).unwrap();
        assert_eq!(undone["title"], "Changed");
        assert!(undone["completions"].as_object().unwrap().is_empty());
        let original = call(&path, "fi", json!({"action":"undo"})).unwrap();
        assert_eq!(original["title"], "Original");
        assert_eq!(original["tags"], json!(["keep"]));
        let req = json!({"action":"undo","request_id":Uuid::new_v4().to_string(),"actor":"owner"});
        let deleted = request(&path, req.clone(), "owner", "fi").unwrap();
        assert_eq!(deleted["deleted"], true);
        assert_eq!(request(&path, req, "owner", "fi").unwrap(), deleted);
        assert!(tasks(&connect(&path).unwrap()).unwrap().is_empty());
        assert!(call(&path, "fi", json!({"action":"undo"})).is_err());
    }
    #[test]
    fn undo_purge_restores_and_refuses_external_edits() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("undo.sqlite3");
        let task = call(
            &path,
            "fi",
            json!({"action":"create","fields":{"title":"Keep me"}}),
        )
        .unwrap();
        call(
            &path,
            "fi",
            json!({"action":"purge","id":task["id"],"revision":task["revision"]}),
        )
        .unwrap();
        let restored = call(&path, "fi", json!({"action":"undo"})).unwrap();
        assert_eq!(restored["id"], task["id"]);
        let edited=call(&path,"li",json!({"action":"edit","id":restored["id"],"revision":restored["revision"],"fields":{"title":"Other device"}})).unwrap();
        assert!(call(&path, "fi", json!({"action":"undo"}))
            .unwrap_err()
            .to_string()
            .contains("changed"));
        assert_eq!(tasks(&connect(&path).unwrap()).unwrap(), vec![edited]);
    }
}

#[cfg(test)]
mod manual_move_tests {
    use super::*;
    fn call(path: &Path, mut value: Value) -> Result<Value> {
        value["request_id"] = json!(Uuid::new_v4().to_string());
        value["actor"] = json!("owner");
        request(path, value, "owner", "fi")
    }
    #[test]
    fn moves_are_ordered_atomic_revision_checked_and_undoable() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("move.db");
        let mut rows = Vec::new();
        for title in ["First", "Second", "Third"] {
            rows.push(
                call(
                    &path,
                    json!({"action":"create","fields":{"title":title,"group":"Work"}}),
                )
                .unwrap(),
            );
        }
        rows.sort_by_key(model::manual_key);
        let moved=call(&path,json!({"action":"move","id":rows[2]["id"],"revision":rows[2]["revision"],"group":"Work","before_id":rows[0]["id"]})).unwrap();
        assert!(model::manual_key(&moved) < model::manual_key(&rows[0]));
        assert!(call(
            &path,
            json!({"action":"move","id":moved["id"],"revision":rows[2]["revision"],"group":"Tech"})
        )
        .is_err());
        let cross=call(&path,json!({"action":"move","id":moved["id"],"revision":moved["revision"],"group":"Tech","before_id":""})).unwrap();
        assert_eq!(cross["group"], "Tech");
        assert!(call(&path,json!({"action":"move","id":rows[0]["id"],"revision":rows[0]["revision"],"group":"Work","before_id":cross["id"]})).is_err());
        let undone = call(&path, json!({"action":"undo"})).unwrap();
        assert_eq!(undone["group"], "Work");
        assert_eq!(undone["manualOrder"], moved["manualOrder"]);
        let restored = call(&path, json!({"action":"undo"})).unwrap();
        assert_eq!(model::manual_key(&restored), model::manual_key(&rows[2]));
    }
}
