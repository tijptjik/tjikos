//! Read-only persistent replica for interactive clients. Writes go to the authority.
use crate::{
    client::{self, Todo},
    client_util::*,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct CachedTodo {
    direct: Todo,
    worker: Option<JoinHandle<Result<()>>>,
    last_attempt: Option<Instant>,
    authority: String,
    error: String,
}
fn read(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({"tasks":[],"updated_at":null}));
    }
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(value["tasks"].is_array(), "Invalid task cache");
    Ok(value)
}
fn save(path: &Path, tasks: Value) -> Result<()> {
    ensure!(tasks.is_array(), "Invalid Tasktjik task list");
    atomic(
        path,
        &serde_json::to_vec(&json!({"tasks":tasks,"updated_at":now()}))?,
    )
}
fn refresh(path: &Path, fetch: impl FnOnce() -> Result<Value>) -> Result<()> {
    // Serialize remote reads and writes across launcher processes. Readers never lock.
    let _lock = client::lock(&path.with_extension("lock"))?;
    save(path, fetch()?)
}
impl CachedTodo {
    pub fn state(&mut self, request: Value, settings: &Value, demo: bool) -> Result<Value> {
        let action = request["action"].as_str().unwrap_or("list");
        let options = if demo || action == "plans" {
            json!({})
        } else {
            client::options(settings)?
        };
        // Explicit local authorities keep their synchronous storage semantics.
        // A configured remote authority never falls back to local storage.
        if demo
            || action == "plans"
            || (s(&options["url"]).is_empty() && s(&options["socket"]).is_empty())
        {
            let mutation = !["list", "refresh", "plans", "open-reference"].contains(&action);
            let mut request = request.clone();
            if action == "refresh" {
                request["action"] = json!("list");
            }
            let mut state = self.direct.state(request, settings, demo)?;
            state["mutation_accepted"] = json!(mutation);
            return Ok(state);
        }
        let identity = json!([options["url"], options["socket"], client::device()?]).to_string();
        let root = xdg("cache").parent().unwrap().join("tasktjik");
        private_dir(&root)?;
        let path: PathBuf = root.join(format!("{:x}.json", Sha256::digest(identity.as_bytes())));
        if self.authority != identity {
            // An old worker may finish its own isolated snapshot safely.
            self.worker = None;
            self.last_attempt = None;
            self.error.clear();
            self.authority = identity;
        }
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            self.error = match self.worker.take().unwrap().join() {
                Ok(Ok(())) => String::new(),
                Ok(Err(e)) => e.to_string(),
                Err(_) => "Task synchronization stopped unexpectedly".into(),
            };
        }
        let mutation = !["list", "refresh", "open-reference"].contains(&action);
        if mutation {
            let _lock = client::lock(&path.with_extension("lock"))?;
            let result = client::api(&options, request.clone())?;
            // Update the replica from the acknowledged task immediately; a complete
            // refresh follows to reconcile other devices and deletions.
            let mut snapshot = read(&path).unwrap_or_else(|_| json!({"tasks":[]}));
            if result.is_object() && result["id"].is_string() {
                let tasks = snapshot["tasks"].as_array_mut().context("Invalid cache")?;
                tasks.retain(|t| t["id"] != result["id"]);
                if result["deleted"] != true {
                    tasks.push(result);
                }
                // Preserve the last full-sync timestamp for partial updates.
                if let Err(e) = atomic(&path, &serde_json::to_vec(&snapshot)?) {
                    self.error = format!("Saved on server; cache update failed: {e}");
                }
            }
            self.last_attempt = None;
        }
        let snapshot = match read(&path) {
            Ok(v) => v,
            Err(e) => {
                self.error = format!("Rebuilding unreadable cache: {e}");
                json!({"tasks":[]})
            }
        };
        if self.worker.is_none()
            && self
                .last_attempt
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(15))
        {
            let path = path.clone();
            self.last_attempt = Some(Instant::now());
            self.worker = Some(thread::spawn(move || {
                refresh(&path, || client::api(&options, json!({"action":"list"})))
            }));
        }
        let mut state = client::render_state(snapshot["tasks"].clone(), &request, false)?;
        state["sync"] = json!({"refreshing":self.worker.is_some(),"updated_at":snapshot["updated_at"],"error":self.error});
        state["mutation_accepted"] = json!(mutation);
        state["sync_message"] = json!(if !self.error.is_empty() {
            format!(
                "Offline or sync failed — showing cached tasks. {}",
                self.error
            )
        } else if snapshot["updated_at"].is_null() {
            "Loading tasks from Tasktjik…".into()
        } else {
            String::new()
        });
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn polling_is_not_a_mutation_and_demo_edits_are_acknowledged() {
        let mut todo = CachedTodo::default();
        let saved = todo.state(json!({"action":"create","fields":{"title":"Read fixture","kind":"reading","reference":"https://example.org"}}), &json!({}), true).unwrap();
        assert_eq!(saved["mutation_accepted"], true);
        let polled = todo
            .state(json!({"action":"refresh"}), &json!({}), true)
            .unwrap();
        assert_eq!(polled["mutation_accepted"], false);
        assert_eq!(polled["tasks"][0]["title"], "Read fixture");
    }
    #[test]
    fn refresh_and_mutation_share_a_cross_process_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let held = client::lock(&path.with_extension("lock")).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            refresh(&path, || {
                tx.send(()).unwrap();
                Ok(json!([]))
            })
        });
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(held);
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.join().unwrap().unwrap();
    }
    #[test]
    fn snapshot_survives_offline_and_reconciles_deletions_and_reading() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        assert!(read(&path).unwrap()["updated_at"].is_null());
        refresh(&path, || {
            Ok(json!([{"id":"a","kind":"todo"},{"id":"b","kind":"reading"}]))
        })
        .unwrap();
        let previous = fs::read(&path).unwrap();
        assert!(refresh(&path, || anyhow::bail!("offline")).is_err());
        assert_eq!(previous, fs::read(&path).unwrap());
        refresh(&path, || Ok(json!([{ "id":"b","kind":"reading" }]))).unwrap();
        assert_eq!(read(&path).unwrap()["tasks"].as_array().unwrap().len(), 1);
        assert!(refresh(&path, || Ok(json!({"invalid":true}))).is_err());
        assert_eq!(read(&path).unwrap()["tasks"][0]["id"], "b");
    }
    #[test]
    fn cached_reads_do_not_wait_for_network_or_writer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        save(&path, json!([])).unwrap();
        let lock = client::lock(&path.with_extension("lock")).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || tx.send(read(&path).unwrap()).unwrap());
        assert!(rx.recv_timeout(Duration::from_secs(1)).is_ok());
        drop(lock);
    }
}
