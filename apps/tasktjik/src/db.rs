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
// Advance only exact historical snapshots after undo changes their revision.
// This keeps consecutive single-task and group undos usable without hiding external edits.
fn advance_undo(
    c: &Connection,
    owner: &str,
    device: &str,
    old: &Value,
    restored: &Value,
) -> Result<()> {
    if old.is_null() || restored["deleted"] == true {
        return Ok(());
    }
    let mut old = old.clone();
    if old.is_object() {
        model::defaults(&mut old);
    }
    let entries: Vec<(i64, String)> = c
        .prepare("SELECT seq,after_body FROM undo_history WHERE owner=? AND device=?")?
        .query_map(params![owner, device], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (seq, body) in entries {
        let mut snapshot: Value = serde_json::from_str(&body)?;
        if snapshot.is_object() {
            model::defaults(&mut snapshot);
        } else if let Some(batch) = snapshot.as_array_mut() {
            for item in batch {
                if item.is_object() {
                    model::defaults(item);
                }
            }
        }
        let mut updated = false;
        if snapshot == old {
            snapshot = restored.clone();
            updated = true;
        } else if let Some(batch) = snapshot.as_array_mut() {
            for item in batch {
                if *item == old {
                    *item = restored.clone();
                    updated = true;
                }
            }
        }
        if updated {
            c.execute(
                "UPDATE undo_history SET after_body=? WHERE seq=?",
                params![serde_json::to_string(&snapshot)?, seq],
            )?;
        }
    }
    Ok(())
}
pub fn request(path: &Path, r: Value, owner: &str, device: &str) -> Result<Value> {
    let mut c = connect(path)?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let op = r["action"].as_str().unwrap_or("list");
    if op == "capabilities" {
        return Ok(
            json!({"reading":true,"idempotent_create":true,"agent_tasks":true,"metadata":true,"images":true,"projects":true,"workflows":true,"undated_repeat":true,"groups":true,"workflow_dependencies":true,"protocol":1,"sqlite":true,"undo":true}),
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
        if previous.is_array() {
            for snapshot in [&mut previous, &mut expected] {
                for task in snapshot.as_array_mut().context("Invalid group undo")? {
                    if task.is_object() {
                        model::defaults(task);
                    }
                }
            }
            let previous = previous.as_array().context("Invalid group undo")?;
            let expected = expected.as_array().context("Invalid group undo")?;
            let mut changed = Vec::new();
            for (old, after) in previous.iter().zip(expected) {
                let current = before_tasks
                    .iter()
                    .find(|t| t["id"] == after["id"])
                    .cloned()
                    .unwrap_or(Value::Null);
                ensure!(
                    current == *after,
                    "Task or groups changed since this action; undo would overwrite newer changes"
                );
                let mut restored = old.clone();
                if restored.is_null() {
                    restored = json!({"id":after["id"],"deleted":true});
                } else {
                    restored["revision"] =
                        json!(after["revision"].as_u64().context("Invalid revision")? + 1);
                    restored["updatedAt"] = json!(model::now());
                }
                advance_undo(&tx, owner, device, old, &restored)?;
                changed.push(restored);
            }
            tx.execute("DELETE FROM undo_history WHERE seq=?", [seq])?;
            json!({"changed":changed})
        } else {
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
            ensure!(
                current["kind"] != "project"
                    || previous["kind"] == "project"
                    || !before_tasks.iter().any(|t| t["project"] == id),
                "Project has tasks; move them before undoing its creation"
            );
            if let Some(project) = previous["project"]
                .as_str()
                .filter(|p| Uuid::parse_str(p).is_ok())
            {
                ensure!(
                    before_tasks
                        .iter()
                        .any(|p| p["id"] == project && p["kind"] == "project"),
                    "Project no longer exists; restore it before undoing this task"
                );
            }
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
            let old: Value = serde_json::from_str(&before)?;
            advance_undo(&tx, owner, device, &old, &restored)?;
            restored
        }
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
        if let Some(changed) = result["changed"].as_array() {
            let before: Vec<Value> = changed
                .iter()
                .map(|t| {
                    before_tasks
                        .iter()
                        .find(|old| old["id"] == t["id"])
                        .cloned()
                        .unwrap_or(Value::Null)
                })
                .collect();
            tx.execute("INSERT INTO undo_history(owner,device,task_id,before_body,after_body) VALUES (?,?,?,?,?)",
                params![owner,device,tasktjik_api::groups::ID,serde_json::to_string(&before)?,serde_json::to_string(changed)?])?;
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
            "subtask-toggle",
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
    let changes = result["changed"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![result.clone()]);
    for change in changes {
        if change["deleted"] == true {
            tx.execute("DELETE FROM tasks WHERE id=?", [change["id"].as_str()])?;
        } else if change["id"].is_string() {
            tx.execute(
                "INSERT INTO tasks VALUES (?,?) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
                params![change["id"].as_str(), serde_json::to_string(&change)?],
            )?;
        }
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

#[cfg(test)]
mod project_workflow_tests {
    use super::*;
    use uuid::Uuid;
    #[test]
    fn undo_cannot_orphan_project_members_across_devices() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("tasks.db");
        let call = |device: &str, mut r: Value| {
            r["request_id"] = json!(Uuid::new_v4().to_string());
            request(&path, r, "test", device)
        };
        let project = call(
            "a",
            json!({"action":"create","fields":{"title":"Synthetic project","kind":"project"}}),
        )
        .unwrap();
        let member = call("b", json!({"action":"create","fields":{"title":"Synthetic member","project":project["id"]}})).unwrap();
        assert!(call("a", json!({"action":"undo"})).is_err());
        call(
            "b",
            json!({"action":"purge","id":member["id"],"revision":member["revision"]}),
        )
        .unwrap();
        call(
            "a",
            json!({"action":"purge","id":project["id"],"revision":project["revision"]}),
        )
        .unwrap();
        assert!(call("b", json!({"action":"undo"})).is_err());
        call("a", json!({"action":"undo"})).unwrap();
        call("b", json!({"action":"undo"})).unwrap();
        assert_eq!(tasks(&connect(&path).unwrap()).unwrap().len(), 2);
    }

    #[test]
    fn projects_templates_revision_checks_and_undo_share_the_authority() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("tasks.db");
        let call = |mut r: Value| {
            r["request_id"] = json!(Uuid::new_v4().to_string());
            request(&path, r, "test", "device")
        };
        let project=call(json!({"action":"create","fields":{"title":"Synthetic project","kind":"project","group":"Work"}})).unwrap();
        let template=call(json!({"action":"create","fields":{"title":"Synthetic template","kind":"template","subtasks":[{"id":"step","kind":"task","title":"First step","due":"","done":false,"items":[]}]}})).unwrap();
        let task=call(json!({"action":"create","template_id":template["id"],"fields":{"title":"Synthetic member","project":project["id"]}})).unwrap();
        let step = task["subtasks"][0]["id"].clone();
        assert_ne!(step, "step");
        let toggle = json!({"action":"subtask-toggle","id":task["id"],"revision":task["revision"],"subtask_id":step});
        let changed = call(toggle.clone()).unwrap();
        assert_eq!(changed["subtasks"][0]["done"], true);
        assert!(call(toggle).is_err());
        let restored = call(json!({"action":"undo"})).unwrap();
        assert_eq!(restored["subtasks"][0]["done"], false);
        let moved=call(json!({"action":"move","id":project["id"],"revision":project["revision"],"group":"Socials"})).unwrap();
        assert_eq!(moved["group"], "Socials");
        assert!(
            call(json!({"action":"purge","id":project["id"],"revision":moved["revision"]}))
                .is_err()
        );
        assert!(call(
            json!({"action":"create","fields":{"title":"Bad member","project":"missing"}})
        )
        .is_err());
        let stored = tasks(&connect(&path).unwrap()).unwrap();
        assert_eq!(
            stored.iter().find(|t| t["id"] == template["id"]).unwrap()["subtasks"][0]["done"],
            false
        );
    }
}

#[cfg(test)]
mod group_tests {
    use super::*;
    fn run(path: &Path, mut r: Value) -> Result<Value> {
        if r["request_id"].is_null() {
            r["request_id"] = json!(Uuid::new_v4().to_string());
        }
        request(path, r, "fixture", "groups-test")
    }
    fn rows(path: &Path) -> Vec<Value> {
        tasks(&connect(path).unwrap()).unwrap()
    }
    fn members(tasks: &[Value], group: &str) -> Value {
        json!(tasks
            .iter()
            .filter(|t| t["group"] == group)
            .map(|t| json!({"id":t["id"],"revision":t["revision"]}))
            .collect::<Vec<_>>())
    }
    #[test]
    fn scoped_rename_preserves_hidden_tasks_and_dates_then_merge_is_atomic_and_undoable() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tasks.db");
        let a = run(&path,json!({"action":"create","fields":{"title":"Visible","group":"Work","due":"2099-11-20"}})).unwrap();
        let b = run(
            &path,
            json!({"action":"create","fields":{"title":"Hidden","group":"Work"}}),
        )
        .unwrap();
        let r = json!({"action":"group-rename","source":"Work","target":"Wedding","scoped":true,"groupsRevision":0,"members":[{"id":a["id"],"revision":a["revision"]}],"request_id":Uuid::new_v4().to_string()});
        let renamed = run(&path, r.clone()).unwrap();
        assert_eq!(run(&path, r).unwrap(), renamed);
        let ts = rows(&path);
        assert_eq!(
            ts.iter().find(|t| t["id"] == b["id"]).unwrap()["group"],
            "Work"
        );
        assert_eq!(renamed["changed"][0]["due"], "2099-11-20");
        assert!(tasktjik_api::groups::names(&ts).contains(&"Work".into()));
        let stale = json!({"action":"group-merge","source":"Wedding","target":"Quick","groupsRevision":1,"members":[{"id":a["id"],"revision":a["revision"]}]});
        assert!(run(&path, stale).is_err());
        assert_eq!(rows(&path), ts);
        let merged=run(&path,json!({"action":"group-merge","source":"Wedding","target":"Quick","groupsRevision":1,"members":members(&ts,"Wedding")})).unwrap();
        assert_eq!(merged["changed"][0]["due"], "2099-11-20");
        assert!(!tasktjik_api::groups::names(&rows(&path)).contains(&"Wedding".into()));
        let undone = run(&path, json!({"action":"undo"})).unwrap();
        assert_eq!(undone["changed"][0]["group"], "Wedding");
        let ts = rows(&path);
        let rev = tasktjik_api::groups::revision(&ts);
        let removed=run(&path,json!({"action":"group-remove","source":"Wedding","groupsRevision":rev,"members":members(&ts,"Wedding")})).unwrap();
        assert_eq!(removed["changed"][0]["group"], "All");
        let task = &removed["changed"][0];
        request(&path,json!({"action":"edit","id":task["id"],"revision":task["revision"],"fields":{"title":"Other device edit"},"request_id":Uuid::new_v4().to_string()}),"fixture","other").unwrap();
        let before = rows(&path);
        assert!(run(&path, json!({"action":"undo"})).is_err());
        assert_eq!(rows(&path), before);
    }
    #[test]
    fn empty_groups_persist_and_full_operations_reject_changed_membership() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tasks.db");
        run(
            &path,
            json!({"action":"group-add","target":"Empty","groupsRevision":0}),
        )
        .unwrap();
        let ts = rows(&path);
        let state = tasktjik_api::client::render_state(json!(ts), &json!({}), true).unwrap();
        assert_eq!(state["tasks"], json!([]));
        assert!(state["groups"]
            .as_array()
            .unwrap()
            .contains(&json!("Empty")));
        assert!(run(
            &path,
            json!({"action":"group-add","target":"Lost","groupsRevision":0})
        )
        .is_err());
        run(
            &path,
            json!({"action":"create","fields":{"title":"New member","group":"Empty"}}),
        )
        .unwrap();
        let before = rows(&path);
        assert!(run(
            &path,
            json!({"action":"group-remove","source":"Empty","groupsRevision":1,"members":[]})
        )
        .is_err());
        assert_eq!(rows(&path), before);
        assert!(run(
            &path,
            json!({"action":"group-remove","source":"All","groupsRevision":1,"members":[]})
        )
        .is_err());
    }
}

#[cfg(test)]
mod consecutive_group_undo {
    use super::*;
    #[test]
    fn consecutive_group_undos_advance_snapshots() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db");
        let run = |mut r: Value| {
            r["request_id"] = json!(Uuid::new_v4().to_string());
            request(&path, r, "user", "device").unwrap()
        };
        run(json!({"action":"group-add","target":"A","groupsRevision":0}));
        run(json!({"action":"group-add","target":"B","groupsRevision":1}));
        run(json!({"action":"undo"}));
        run(json!({"action":"undo"}));
        assert!(
            !tasktjik_api::groups::names(&tasks(&connect(&path).unwrap()).unwrap())
                .contains(&"A".to_owned())
        );
    }
}
