//! Pure native task projection. No storage or network I/O occurs here.
use serde_json::{json, Value};

pub fn project(state: &Value, request: &Value) -> Result<Value, String> {
    project_inner(state, request).map_err(|e| format!("{e:#}"))
}
fn project_inner(state: &Value, request: &Value) -> anyhow::Result<Value> {
    let action = request["action"].as_str().unwrap_or("");
    if ![
        "create",
        "edit",
        "toggle",
        "done",
        "status",
        "purge",
        "move",
        "subtask-toggle",
        "subtask-cancel",
        "snooze",
        "verify",
        "attach-image",
        "group-add",
        "group-rename",
        "group-merge",
        "group-remove",
        "reading-save",
    ]
    .contains(&action)
    {
        return Ok(state.clone());
    }
    let mut rows = state["tasks"].as_array().cloned().unwrap_or_default();
    rows.push(json!({"id":crate::groups::ID,"kind":"settings","groups":state["groups"],"revision":state["groupsRevision"].as_u64().unwrap_or(0)}));
    let mut command = request.clone();
    if action == "reading-save" {
        command["action"] = json!("create");
    }
    let mut changed = crate::model::mutate(&rows, &command)?;
    if command["action"] == "create" {
        changed["id"] = request["_local_id"].clone();
        if action == "reading-save" {
            changed["createKey"] = request["draft_id"].clone();
        }
    }
    let changes = changed["changed"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![changed]);
    crate::groups::apply(&mut rows, &changes);
    let rendered = crate::client::render_state(json!(rows), &json!({"action":"list"}), true)?;
    let mut result = state.clone();
    for (key, value) in rendered.as_object().unwrap() {
        result[key] = value.clone();
    }
    if action == "reading-save" {
        result["draft_id"] = json!("");
        result["draft"] = Value::Null;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_uses_authority_completion_workflow_and_group_rules() {
        let task = crate::model::mutate(&[], &json!({"action":"create","fields":{"title":"Synthetic","subtasks":[{"id":"step","kind":"task","title":"Step","due":"","items":[],"done":false}]}})).unwrap();
        let state = crate::client::render_state(json!([task]), &json!({}), true).unwrap();
        let task = &state["tasks"][0];
        let done = project(
            &state,
            &json!({"action":"toggle","id":task["id"],"revision":task["revision"]}),
        )
        .unwrap();
        assert_eq!(done["tasks"][0]["done"], true);
        let step = project(&state, &json!({"action":"subtask-toggle","id":task["id"],"revision":task["revision"],"subtask_id":"step"})).unwrap();
        assert_eq!(step["tasks"][0]["subtaskProgress"]["done"], 1);
        let renamed = project(&state, &json!({"action":"group-rename","groupsRevision":0,"source":"All","target":"New","scoped":true,"members":[{"id":task["id"],"revision":task["revision"]}]})).unwrap();
        assert_eq!(renamed["tasks"][0]["group"], "New");
        let created = project(&state, &json!({"action":"create","request_id":"create-key","_local_id":"local","fields":{"title":"New"}})).unwrap();
        assert!(created["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == "local"));
    }
}
