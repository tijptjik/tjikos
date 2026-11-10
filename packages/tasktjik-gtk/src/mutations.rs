//! Ordered writes over an authoritative snapshot. Pending intent is replayed after
//! each acknowledgement, so an older response cannot erase newer local edits.
use serde_json::{json, Value};
use std::{collections::VecDeque, rc::Rc};
pub type Projector = Rc<dyn Fn(&Value, &Value) -> Result<Value, String>>;
#[derive(Default)]
pub struct Mutations {
    pub base: Value,
    pub queue: VecDeque<Value>,
    pub projector: Option<Projector>,
    pub history: Vec<(String, Value, Value)>,
    pub failed: Vec<Value>,
}
fn tasks(state: &Value) -> Vec<Value> {
    state["tasks"].as_array().cloned().unwrap_or_default()
}
impl Mutations {
    pub fn visible(&self) -> Value {
        let mut state = self.base.clone();
        for request in &self.queue {
            if let Some(restore) = request.get("_undo") {
                let mut rows = tasks(&state);
                for item in restore.as_array().into_iter().flatten() {
                    rows.retain(|t| t["id"] != item["id"]);
                    if item["deleted"] != true {
                        rows.push(item.clone());
                    }
                }
                state["tasks"] = json!(rows);
                if let Some(groups) = request["_undo_groups"].as_object() {
                    for (key, value) in groups {
                        state[key] = value.clone();
                    }
                }
            } else if let Some(project) = &self.projector {
                let mut local = request.clone();
                Self::revision(&state, &mut local);
                if let Ok(next) = project(&state, &local) {
                    state = next;
                }
            }
        }
        state
    }
    pub fn revision(state: &Value, request: &mut Value) {
        if let Some(task) = tasks(state).iter().find(|t| t["id"] == request["id"]) {
            request["revision"] = task["revision"].clone();
        }
        if request["action"]
            .as_str()
            .unwrap_or("")
            .starts_with("group-")
        {
            request["groupsRevision"] = state["groupsRevision"].clone();
            if let Some(members) = request["members"].as_array_mut() {
                for member in members {
                    if let Some(task) = tasks(state).iter().find(|t| t["id"] == member["id"]) {
                        member["revision"] = task["revision"].clone();
                    }
                }
            }
        }
    }
    pub fn enqueue(&mut self, mut request: Value) -> Result<(), String> {
        let before = self.visible();
        if request["action"] == "undo" {
            if let Some((key, old, new)) = self.history.pop() {
                request["_undo_of"] = json!(key);
                request["_undo_groups"] = old["group_state"].clone();
                request["id"] = new["undo_target"].clone();
                if request["id"].is_null() {
                    request.as_object_mut().unwrap().remove("id");
                }
                request["_undo_history"] = json!([key, old, new]);
                let old_rows = tasks(&old);
                let new_rows = tasks(&new);
                let mut restore: Vec<Value> = old_rows
                    .iter()
                    .filter(|t| !new_rows.contains(t))
                    .cloned()
                    .collect();
                restore.extend(
                    new_rows
                        .iter()
                        .filter(|t| !old_rows.iter().any(|o| o["id"] == t["id"]))
                        .map(|t| json!({"id":t["id"],"deleted":true})),
                );
                request["_undo"] = json!(restore);
            }
        } else if let Some(project) = &self.projector {
            let after = project(&before, &request)?;
            if before != after {
                self.history.push(history_entry(&request, &before, &after));
            }
        }

        if self.history.len() > 32 {
            self.history.remove(0);
        }
        self.queue.push_back(request);
        Ok(())
    }
    pub fn ingest(&mut self, value: &Value) {
        if !self.base.is_object() {
            self.base = json!({});
        }
        if let Some(map) = value.as_object() {
            for (k, v) in map {
                if ![
                    "source",
                    "mutation_accepted",
                    "retryable_error",
                    "collection",
                    "gtk_view",
                ]
                .contains(&k.as_str())
                {
                    self.base[k] = v.clone();
                }
            }
        }
    }
    pub fn resolve(&mut self, id: &str, value: Option<&Value>) -> bool {
        if self.queue.front().is_none_or(|r| r["request_id"] != id) {
            return false;
        }
        let finished = self.queue.pop_front().unwrap();
        let mut affected: std::collections::BTreeSet<String> = finished["id"]
            .as_str()
            .into_iter()
            .map(str::to_owned)
            .collect();
        for member in finished["members"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(finished["_undo"].as_array().into_iter().flatten())
        {
            if let Some(id) = member["id"].as_str() {
                affected.insert(id.into());
            }
        }
        if let Some(value) = value {
            self.ingest(value);
            if let Some(local) = finished["_local_id"].as_str() {
                let key = if finished["action"] == "reading-save" {
                    &finished["draft_id"]
                } else {
                    &finished["request_id"]
                };
                if let Some(created) = tasks(&self.base).iter().find(|t| &t["createKey"] == key) {
                    let actual = created["id"].clone();
                    if let Some(id) = actual.as_str() {
                        affected.insert(id.into());
                    }
                    for request in &mut self.queue {
                        replace_id(request, local, &actual);
                    }
                    for (_, before, after) in &mut self.history {
                        replace_id(before, local, &actual);
                        replace_id(after, local, &actual);
                    }
                }
            }
        } else {
            self.history.retain(|(key, _, _)| key != id);
            // An undo of a rejected write must never undo an unrelated earlier write.
            self.queue.retain(|r| r["_undo_of"] != id);
            if let Ok(entry) =
                serde_json::from_value::<(String, Value, Value)>(finished["_undo_history"].clone())
            {
                self.history.push(entry);
            }
            if finished["fields"].is_object() {
                self.failed.push(finished.clone());
            }
        }
        // Only refresh revisions for our queued lineage after its own response.
        // The first write still carries the revision the user actually edited.
        let mut projected = self.base.clone();
        for next in &mut self.queue {
            if next["id"].as_str().is_some_and(|id| affected.contains(id)) {
                Self::revision(&projected, next);
            }
            if finished["action"]
                .as_str()
                .unwrap_or("")
                .starts_with("group-")
                && next["action"].as_str().unwrap_or("").starts_with("group-")
            {
                next["groupsRevision"] = projected["groupsRevision"].clone();
            }
            if let Some(project) = &self.projector {
                let mut local = next.clone();
                Self::revision(&projected, &mut local);
                if let Ok(state) = project(&projected, &local) {
                    if let Some(entry) = self
                        .history
                        .iter_mut()
                        .find(|(key, _, _)| next["request_id"] == key.as_str())
                    {
                        *entry = history_entry(next, &projected, &state);
                    }
                    projected = state;
                }
            }
        }
        true
    }
    pub fn wire(&self) -> Option<Value> {
        let mut request = self.queue.front()?.clone();
        request.as_object_mut()?.retain(|k, _| !k.starts_with('_'));
        Some(request)
    }
}
fn history_entry(request: &Value, before: &Value, after: &Value) -> (String, Value, Value) {
    let old_rows = tasks(before);
    let new_rows = tasks(after);
    let changed: std::collections::BTreeSet<String> = old_rows
        .iter()
        .chain(new_rows.iter())
        .filter(|t| !old_rows.contains(t) || !new_rows.contains(t))
        .map(|t| t["id"].as_str().unwrap_or("").to_owned())
        .collect();
    let undo_target = if request["action"]
        .as_str()
        .unwrap_or("")
        .starts_with("group-")
    {
        json!("00000000-0000-4000-8000-000000000001")
    } else {
        request
            .get("_local_id")
            .cloned()
            .unwrap_or_else(|| request["id"].clone())
    };
    let groups = if before["groups"] != after["groups"] {
        json!({"groups":before["groups"],"groupsRevision":before["groupsRevision"]})
    } else {
        Value::Null
    };
    (
        request["request_id"].as_str().unwrap_or("").into(),
        json!({"group_state":groups,"tasks":old_rows.into_iter().filter(|t| changed.contains(t["id"].as_str().unwrap_or(""))).collect::<Vec<_>>()}),
        json!({"undo_target":undo_target,"tasks":new_rows.into_iter().filter(|t| changed.contains(t["id"].as_str().unwrap_or(""))).collect::<Vec<_>>()}),
    )
}
fn replace_id(value: &mut Value, old: &str, new: &Value) {
    match value {
        Value::Array(a) => {
            for v in a {
                replace_id(v, old, new);
            }
        }
        Value::Object(o) => {
            for (key, v) in o.iter_mut() {
                if [
                    "id",
                    "project",
                    "task_id",
                    "before_id",
                    "undo_target",
                    "_local_id",
                ]
                .contains(&key.as_str())
                    && v.as_str() == Some(old)
                {
                    *v = new.clone();
                } else {
                    replace_id(v, old, new);
                }
            }
        }
        _ => {}
    }
}

impl Mutations {
    pub fn snapshot(&self) -> Value {
        json!({"base":self.base,"queue":self.queue,"history":self.history,"failed":self.failed})
    }
    pub fn restore(&mut self, value: &Value) -> Result<(), String> {
        self.failed = serde_json::from_value(value.get("failed").cloned().unwrap_or(json!([])))
            .map_err(|e| e.to_string())?;
        self.base = value["base"].clone();
        self.queue = serde_json::from_value(value["queue"].clone()).map_err(|e| e.to_string())?;
        self.history =
            serde_json::from_value(value["history"].clone()).map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn queue() -> Mutations {
        let mut q = Mutations::default();
        q.base = json!({"tasks":[{"id":"a","revision":1,"title":"A","done":false},{"id":"b","revision":4,"title":"B","done":false}]});
        q.projector = Some(Rc::new(|state, request| {
            let mut state = state.clone();
            if request["action"] == "edit" || request["action"] == "toggle" {
                let task = state["tasks"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|t| t["id"] == request["id"])
                    .ok_or("Missing task")?;
                if task["revision"] != request["revision"] {
                    return Err("Conflict".into());
                }
                if request["action"] == "toggle" {
                    task["done"] = json!(task["done"] != true);
                } else {
                    for (key, value) in request["fields"].as_object().unwrap() {
                        task[key] = value.clone();
                    }
                }
                task["revision"] = json!(task["revision"].as_u64().unwrap() + 1);
            }
            Ok(state)
        }));
        q
    }
    #[test]
    fn queued_edits_survive_ack_and_failed_predecessor() {
        let mut q = queue();
        q.enqueue(json!({"action":"toggle","id":"a","revision":1,"request_id":"one"}))
            .unwrap();
        q.enqueue(json!({"action":"edit","id":"a","revision":2,"request_id":"two","fields":{"title":"Changed"}})).unwrap();
        assert_eq!(q.visible()["tasks"][0]["title"], "Changed");
        assert_eq!(q.visible()["tasks"][0]["done"], true);
        assert!(!q.resolve("unrelated", None));
        assert!(q.resolve("one", None));
        assert_eq!(q.visible()["tasks"][0]["done"], false);
        assert_eq!(q.visible()["tasks"][0]["title"], "Changed");
        assert_eq!(q.wire().unwrap()["revision"], 1);
        let saved = q.visible();
        q.resolve("two", Some(&saved));
        assert_eq!(q.visible(), saved);
        assert!(q.queue.is_empty());
    }
    #[test]
    fn successful_ack_does_not_replace_newer_local_edit_or_weaken_other_guards() {
        let mut q = queue();
        q.enqueue(json!({"action":"edit","id":"a","revision":1,"request_id":"one","fields":{"title":"First"}})).unwrap();
        let mut ack = q.visible();
        q.enqueue(json!({"action":"edit","id":"a","revision":2,"request_id":"two","fields":{"title":"Latest"}})).unwrap();
        q.enqueue(json!({"action":"edit","id":"b","revision":4,"request_id":"three","fields":{"title":"Local B"}})).unwrap();
        ack["tasks"][1]["revision"] = json!(5); // another writer changed B
        q.resolve("one", Some(&ack));
        assert_eq!(q.visible()["tasks"][0]["title"], "Latest");
        assert_eq!(
            q.queue[1]["revision"], 4,
            "unrelated revision guard must survive"
        );
        let mut restored = queue();
        restored.restore(&q.snapshot()).unwrap();
        assert_eq!(restored.wire(), q.wire());
        assert_eq!(restored.visible(), q.visible());
    }
    #[test]
    fn rejected_write_cancels_its_undo_and_rebases_later_undo_snapshot() {
        let mut q = queue();
        q.enqueue(json!({"action":"toggle","id":"a","revision":1,"request_id":"one"}))
            .unwrap();
        q.enqueue(json!({"action":"undo","request_id":"undo"}))
            .unwrap();
        assert_eq!(q.queue[1]["id"], "a");
        q.resolve("one", None);
        assert!(
            q.queue.is_empty(),
            "never undo an earlier unrelated action after a failed write"
        );
        q.enqueue(json!({"action":"toggle","id":"a","revision":1,"request_id":"two"}))
            .unwrap();
        q.enqueue(json!({"action":"edit","id":"a","revision":2,"request_id":"three","fields":{"title":"Changed"}})).unwrap();
        q.resolve("two", None);
        q.enqueue(json!({"action":"undo","request_id":"undo-three"}))
            .unwrap();
        let visible = q.visible();
        let restored = visible["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "a")
            .unwrap();
        assert_eq!(restored["done"], false);
        assert_eq!(restored["title"], "A");
    }
    #[test]
    fn create_ack_remaps_followup_references_and_preserves_request_ids() {
        let mut q = queue();
        q.queue
            .push_back(json!({"action":"create","_local_id":"temporary","request_id":"create"}));
        q.queue.push_back(json!({"action":"edit","id":"temporary","revision":1,"fields":{"title":"Edited immediately"},"request_id":"edit"}));
        q.resolve(
            "create",
            Some(&json!({"tasks":[{"id":"real","revision":1,"createKey":"create","title":"New"}]})),
        );
        assert_eq!(q.wire().unwrap()["id"], "real");
        assert_eq!(q.wire().unwrap()["request_id"], "edit");
        assert_eq!(q.visible()["tasks"][0]["title"], "Edited immediately");
    }
}
