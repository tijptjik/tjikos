//! Revision-checked group changes, applied as one batch by the storage backend.
use crate::model;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
pub const ID: &str = "00000000-0000-4000-8000-000000000001";
pub fn names(tasks: &[Value]) -> Vec<String> {
    let mut names: Vec<String> = tasks
        .iter()
        .find(|t| t["id"] == ID)
        .and_then(|t| t["groups"].as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_else(|| {
            [
                "All", "Quick", "General", "Work", "Socials", "Domestic", "Finances", "Tech",
            ]
            .map(str::to_owned)
            .to_vec()
        });
    for t in tasks.iter().filter(|t| t["kind"] != "settings") {
        if let Some(g) = t["group"].as_str() {
            if !names.iter().any(|n| n == g) {
                names.push(g.into());
            }
        }
    }
    names
}
pub fn revision(tasks: &[Value]) -> u64 {
    tasks
        .iter()
        .find(|t| t["id"] == ID)
        .and_then(|t| t["revision"].as_u64())
        .unwrap_or(0)
}
pub fn mutate(tasks: &[Value], r: &Value) -> Result<Value> {
    ensure!(
        r["groupsRevision"].as_u64() == Some(revision(tasks)),
        "Groups changed; refresh before updating"
    );
    let op = r["action"].as_str().unwrap_or("");
    let source = r["source"].as_str().unwrap_or("");
    let target = if op == "group-remove" {
        "All"
    } else {
        r["target"].as_str().unwrap_or("").trim()
    };
    ensure!(
        !target.is_empty() && target.len() <= 100 && !target.chars().any(char::is_control),
        "Choose a group name (up to 100 characters)"
    );
    let mut groups = names(tasks);
    let scoped = r["scoped"] == true;
    ensure!(
        !scoped || op == "group-rename",
        "Only renames can be scoped"
    );
    let mut changed = Vec::new();
    if op == "group-add" {
        ensure!(!groups.iter().any(|g| g == target), "Group already exists");
    } else {
        ensure!(
            ["group-rename", "group-merge", "group-remove"].contains(&op),
            "Unknown group operation"
        );
        ensure!(
            groups.iter().any(|g| g == source) && source != target,
            "Choose different source and destination groups"
        );
        ensure!(
            scoped || source != "All",
            "Inbox cannot be removed or renamed"
        );
        if op == "group-merge" {
            ensure!(
                groups.iter().any(|g| g == target),
                "Destination group no longer exists"
            );
        }
        if op == "group-rename" {
            ensure!(
                !groups.iter().any(|g| g == target),
                "Group exists; use merge in settings"
            );
        }
        let members = r["members"]
            .as_array()
            .context("Group member revisions required")?;
        let actual: Vec<_> = tasks
            .iter()
            .filter(|t| t["kind"] != "settings" && t["group"] == source)
            .collect();
        ensure!(
            scoped || actual.len() == members.len(),
            "Group membership changed; refresh before updating"
        );
        let mut seen = std::collections::HashSet::new();
        for member in members {
            ensure!(
                seen.insert(member["id"].as_str().context("Invalid member ID")?),
                "Duplicate group member"
            );
            let task = actual
                .iter()
                .find(|t| t["id"] == member["id"] && t["revision"] == member["revision"])
                .context("Task or group membership changed; refresh before updating")?;
            let mut task = (*task).clone();
            task["group"] = json!(target);
            task["revision"] = json!(task["revision"]
                .as_u64()
                .context("Invalid revision")?
                .checked_add(1)
                .context("Invalid revision")?);
            task["updatedAt"] = json!(model::now());
            task["events"].as_array_mut().context("Invalid events")?.push(json!({"action":op,"at":model::now(),"actor":r["actor"],"from":source,"to":target}));
            model::validate(&mut task)?;
            changed.push(task);
        }
        if !scoped {
            groups.retain(|g| g != source);
        }
    }
    if !groups.iter().any(|g| g == target) {
        groups.push(target.into());
    }
    let mut registry = json!({"id":ID,"kind":"settings","groups":groups,"revision":revision(tasks)+1,"status":"cancelled","due":"","repeat":{},"completions":{},"notifications":{},"createdAt":model::now(),"updatedAt":model::now()});
    model::defaults(&mut registry);
    changed.push(registry);
    Ok(json!({"changed":changed}))
}
pub fn apply(tasks: &mut Vec<Value>, changes: &[Value]) {
    for change in changes {
        tasks.retain(|t| t["id"] != change["id"]);
        if change["deleted"] != true {
            tasks.push(change.clone());
        }
    }
}
