//! Embedded workflows are updated atomically with their owning task. Cancellation
//! is derived, never written over completion history, so reopening is reversible.
use anyhow::{bail, ensure, Context, Result};
use chrono::{Duration, NaiveDate};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

pub fn offset_days(text: &str) -> Result<i64> {
    let text = text.trim().to_lowercase();
    if ["on deadline", "on the deadline", "same day"].contains(&text.as_str()) {
        return Ok(0);
    }
    let words: Vec<_> = text.split_whitespace().collect();
    ensure!(
        words.len() >= 3 && words.len() <= 6,
        "Use e.g. one week before the deadline"
    );
    let count = match words[0] {
        "a" | "an" | "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        n => n.parse::<i64>().context("Invalid reminder interval")?,
    };
    ensure!((0..=10000).contains(&count), "Invalid reminder interval");
    let unit = match words[1] {
        "day" | "days" => 1,
        "week" | "weeks" => 7,
        _ => bail!("Reminder intervals use days or weeks"),
    };
    ensure!(
        words[2] == "before",
        "Reminders must be before the deadline"
    );
    ensure!(
        ["", "deadline", "the deadline", "due date", "the due date"]
            .contains(&words[3..].join(" ").as_str()),
        "Invalid reminder interval"
    );
    Ok(count * unit)
}

pub fn validate(nodes: &Value) -> Result<()> {
    fn walk(nodes: &Value, choices: bool, depth: usize, ids: &mut HashSet<String>) -> Result<()> {
        ensure!(depth <= 8, "Subtasks may be nested up to eight levels");
        let nodes = nodes.as_array().context("Subtasks must be a list")?;
        let mut selected = 0;
        for n in nodes {
            ensure!(n.is_object(), "Invalid subtask");
            let id = s(n, "id");
            ensure!(
                !id.is_empty() && id.len() <= 100 && ids.insert(id.into()),
                "Subtask IDs must be unique"
            );
            ensure!(ids.len() <= 500, "A task can have up to 500 subtask items");
            ensure!(
                !s(n, "title").trim().is_empty() && s(n, "title").len() <= 4096,
                "Subtask title is required"
            );
            let kind = s(n, "kind");
            ensure!(
                ["task", "flow", "exit", "reminder", "group"].contains(&kind),
                "Invalid subtask kind"
            );
            ensure!(
                !choices || !["flow", "group"].contains(&kind),
                "An alternative cannot be a flow block"
            );
            ensure!(
                choices || !["exit", "reminder"].contains(&kind),
                "Exits and reminders belong to a flow block"
            );
            for flag in ["cancelled", "acceptCancelled"] {
                ensure!(
                    n.get(flag).is_none_or(Value::is_boolean),
                    "Invalid pathway setting"
                );
            }
            ensure!(n["done"].is_boolean(), "Invalid subtask completion");
            ensure!(n["due"].is_string(), "Invalid subtask due date");
            if !s(n, "due").is_empty() {
                NaiveDate::parse_from_str(s(n, "due"), "%Y-%m-%d")?;
            }
            ensure!(
                !choices || s(n, "due").is_empty(),
                "Set deadlines on the flow block"
            );
            if kind == "flow" {
                ensure!(
                    n["done"] == false,
                    "Complete a flow by choosing an alternative"
                );
                ensure!(
                    n["items"].as_array().is_some_and(|a| !a.is_empty()),
                    "A flow needs alternatives"
                );
                let has_reminder = n["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["kind"] == "reminder");
                ensure!(
                    !has_reminder || !s(n, "due").is_empty(),
                    "A reminder needs a flow deadline"
                );
            }
            if kind == "reminder" {
                offset_days(s(n, "offset"))?;
                ensure!(
                    n["items"].as_array().is_some_and(Vec::is_empty),
                    "Reminders cannot activate branches"
                );
            }
            if kind == "exit" {
                ensure!(
                    n["items"].as_array().is_some_and(Vec::is_empty),
                    "Exits cannot activate branches"
                );
            }
            if choices && kind != "reminder" && n["done"] == true {
                selected += 1;
            }
            walk(&n["items"], kind == "flow", depth + 1, ids)?;
        }
        ensure!(
            selected <= 1,
            "Reopen the selected alternative before choosing another"
        );
        Ok(())
    }
    walk(nodes, false, 0, &mut HashSet::new())?;
    // Only earlier, fully visited nodes may be dependencies: no cycles or ancestors.
    fn dependencies(nodes: &Value, available: &mut HashSet<String>, choice: bool) -> Result<()> {
        for n in nodes.as_array().unwrap() {
            if let Some(requires) = n.get("requires") {
                ensure!(
                    !choice,
                    "Set dependencies on the flow, not its alternatives"
                );
                let requires = requires.as_array().context("Dependencies must be a list")?;
                ensure!(!requires.is_empty(), "Choose at least one dependency");
                let mut unique = HashSet::new();
                for id in requires {
                    let id = id.as_str().context("Invalid dependency ID")?;
                    ensure!(available.contains(id) && unique.insert(id), "Dependencies must reference distinct earlier steps outside their own branch");
                }
            }
            if n["kind"] == "flow" || choice {
                // Conditional branch internals are private to that branch. Depend on
                // the enclosing flow to wait for whichever alternative was chosen.
                dependencies(&n["items"], &mut available.clone(), n["kind"] == "flow")?;
            } else {
                dependencies(&n["items"], available, false)?;
            }
            if !choice {
                available.insert(s(n, "id").into());
            }
        }
        Ok(())
    }
    dependencies(nodes, &mut HashSet::new(), false)
}

pub fn instantiate(nodes: &Value) -> Value {
    let mut nodes = nodes.clone();
    fn reset(nodes: &mut Value, ids: &mut HashMap<String, String>) {
        if let Some(list) = nodes.as_array_mut() {
            for node in list {
                let id = Uuid::new_v4().to_string();
                ids.insert(s(node, "id").to_owned(), id.clone());
                node["id"] = json!(id);
                node.as_object_mut().unwrap().remove("cancelled");
                node["done"] = json!(false);
                reset(&mut node["items"], ids);
            }
        }
    }
    let mut ids = HashMap::new();
    reset(&mut nodes, &mut ids);
    fn remap(nodes: &mut Value, ids: &HashMap<String, String>) {
        for node in nodes.as_array_mut().unwrap() {
            if let Some(requires) = node.get_mut("requires").and_then(Value::as_array_mut) {
                for id in requires {
                    *id = json!(ids[id.as_str().unwrap()]);
                }
            }
            remap(&mut node["items"], ids);
        }
    }
    remap(&mut nodes, &ids);
    nodes
}

pub fn rows(nodes: &Value, day: NaiveDate) -> Result<Vec<Value>> {
    rows_with_workflow(nodes, day, false)
}

pub fn task_rows(task: &Value, day: NaiveDate) -> Result<Vec<Value>> {
    rows_with_workflow(
        task.get("subtasks").unwrap_or(&json!([])),
        day,
        task["workflow"] == true,
    )
}

fn rows_with_workflow(nodes: &Value, day: NaiveDate, workflow: bool) -> Result<Vec<Value>> {
    // Returns whether an exit stopped the sequence and whether every active step
    // resolved. Raw completion flags remain untouched when a predecessor reopens.
    fn sequence(
        nodes: &Value,
        day: NaiveDate,
        depth: usize,
        cancelled: bool,
        inherited_blocked: bool,
        sequential: bool,
        out: &mut Vec<Value>,
        states: &mut HashMap<String, (bool, bool)>,
    ) -> Result<(bool, bool)> {
        let mut stopped = cancelled;
        let mut complete = true;
        let mut previous = String::new();
        for n in nodes.as_array().context("Invalid subtasks")? {
            let dependencies = n.get("requires").and_then(Value::as_array);
            let dependency_cancelled = dependencies.is_some_and(|ids| {
                ids.iter().any(|id| {
                    states
                        .get(id.as_str().unwrap())
                        .is_some_and(|state| state.1)
                })
            });
            let dependency_ready = dependencies.is_some_and(|ids| {
                ids.iter().all(|id| {
                    states
                        .get(id.as_str().unwrap())
                        .is_some_and(|state| state.0 || (state.1 && n["acceptCancelled"] == true))
                })
            });
            let blocked = inherited_blocked
                || if dependencies.is_some() {
                    !dependency_ready
                } else {
                    sequential && !complete
                };
            let cancelled = (stopped && !(dependency_ready && n["acceptCancelled"] == true))
                || n["cancelled"] == true
                || (dependency_cancelled && n["acceptCancelled"] != true);
            let blocked = blocked && !cancelled;
            let inherited_stop = stopped;
            stopped = cancelled;
            let start = out.len();
            let mut resolved;
            if n["kind"] == "flow" {
                let choices = n["items"].as_array().context("Invalid alternatives")?;
                let winner = choices
                    .iter()
                    .find(|c| c["kind"] != "reminder" && c["done"] == true);
                let mut header = n.clone();
                header["depth"] = json!(depth);
                header["blocked"] = json!(blocked && !stopped);
                header["dependsOn"] = json!(if sequential && dependencies.is_none() {
                    previous.as_str()
                } else {
                    ""
                });
                out.push(header);
                resolved = winner.is_some();
                for c in choices {
                    let reminder = c["kind"] == "reminder";
                    let due = if reminder {
                        NaiveDate::parse_from_str(s(n, "due"), "%Y-%m-%d")?
                            .checked_sub_signed(Duration::days(offset_days(s(c, "offset"))?))
                            .context("Reminder date out of range")?
                            .to_string()
                    } else {
                        s(n, "due").to_owned()
                    };
                    let cancelled = stopped || winner.is_some_and(|w| w["id"] != c["id"]);
                    let status = if reminder && c["done"] == true {
                        "done"
                    } else if stopped {
                        "cancelled"
                    } else if blocked {
                        "blocked"
                    } else if cancelled {
                        "cancelled"
                    } else if c["done"] == true {
                        "done"
                    } else if reminder && due > day.to_string() {
                        "snoozed"
                    } else {
                        "ready"
                    };
                    let mut row = c.clone();
                    row["due"] = json!(due);
                    row["status"] = json!(status);
                    row["blocked"] = json!(blocked && !stopped);
                    row["depth"] = json!(depth + 1);
                    row["blockId"] = n["id"].clone();
                    out.push(row);
                    if c["done"] == true && !cancelled && !reminder && !blocked {
                        let (exit, branch_complete) = sequence(
                            &c["items"],
                            day,
                            depth + 1,
                            false,
                            false,
                            false,
                            out,
                            states,
                        )?;
                        resolved &= branch_complete;
                        if exit {
                            stopped = true;
                        }
                    }
                }
                out[start]["status"] = json!(if cancelled {
                    "cancelled"
                } else if blocked {
                    "blocked"
                } else if resolved {
                    "done"
                } else {
                    "ready"
                });
                if !blocked && !stopped && winner.is_some_and(|w| w["kind"] == "exit") {
                    stopped = true;
                }
            } else {
                let mut row = n.clone();
                row["status"] = json!(if stopped {
                    "cancelled"
                } else if blocked {
                    "blocked"
                } else if n["done"] == true {
                    "done"
                } else {
                    "ready"
                });
                row["depth"] = json!(depth);
                row["blocked"] = json!(blocked && !stopped);
                row["dependsOn"] = json!(if sequential && dependencies.is_none() {
                    previous.as_str()
                } else {
                    ""
                });
                out.push(row);
                let (exit, children_complete) = sequence(
                    &n["items"],
                    day,
                    depth + 1,
                    stopped,
                    blocked,
                    false,
                    out,
                    states,
                )?;
                resolved = (n["done"] == true || n["kind"] == "group") && children_complete;
                if n["kind"] == "group" && !stopped && !blocked {
                    out[start]["status"] = json!(if resolved { "done" } else { "ready" });
                }
                if exit {
                    stopped = true;
                }
            }
            states.insert(
                s(n, "id").to_owned(),
                (resolved && !blocked && !cancelled, cancelled),
            );
            // Explicit cancellation affects this dependency path only. Flow exits retain legacy behavior.
            if cancelled && !inherited_stop {
                stopped = false;
            }
            if !stopped {
                complete &= cancelled || (resolved && !blocked);
            }
            previous = s(n, "id").to_owned();
        }
        Ok((stopped, complete))
    }
    let mut out = Vec::new();
    sequence(
        nodes,
        day,
        0,
        false,
        false,
        workflow,
        &mut out,
        &mut HashMap::new(),
    )?;
    Ok(out)
}

pub fn toggle(task: &mut Value, id: &str, day: NaiveDate) -> Result<()> {
    let visible = task_rows(task, day)?;
    let row = visible
        .iter()
        .find(|r| r["id"] == id)
        .context("Subtask is not active")?;
    ensure!(
        row["kind"] != "flow" && row["kind"] != "group",
        "Complete the contained steps"
    );
    ensure!(
        row["status"] != "cancelled",
        "Reopen the selected alternative first"
    );
    ensure!(
        row["status"] != "blocked" && row["blocked"] != true,
        "Complete the preceding workflow step first"
    );
    fn change(nodes: &mut Value, id: &str) -> bool {
        for n in nodes.as_array_mut().unwrap() {
            if n["id"] == id {
                n["done"] = json!(n["done"] != true);
                return true;
            }
            if change(&mut n["items"], id) {
                return true;
            }
        }
        false
    }
    ensure!(
        change(&mut task["subtasks"], id),
        "Subtask no longer exists"
    );
    Ok(())
}

pub fn cancel(task: &mut Value, id: &str, day: NaiveDate) -> Result<()> {
    let rows = task_rows(task, day)?;
    let row = rows
        .iter()
        .find(|r| r["id"] == id)
        .context("Subtask is not active")?;
    ensure!(
        row.get("blockId").is_none(),
        "Cancel the flow instead of an alternative"
    );
    ensure!(
        row["cancelled"] == true || (row["status"] != "blocked" && row["status"] != "cancelled"),
        "This pathway is not available"
    );
    fn change(nodes: &mut Value, id: &str) -> bool {
        for n in nodes.as_array_mut().unwrap() {
            if n["id"] == id {
                n["cancelled"] = json!(n["cancelled"] != true);
                return true;
            }
            if change(&mut n["items"], id) {
                return true;
            }
        }
        false
    }
    ensure!(
        change(&mut task["subtasks"], id),
        "Subtask no longer exists"
    );
    Ok(())
}

pub fn decorate(task: &mut Value, day: NaiveDate) -> Result<()> {
    let rows = task_rows(task, day)?;
    // A flow is one stage, rather than counting mutually exclusive choices as work.
    let pips: Vec<Value> = rows
        .iter()
        .filter(|r| r.get("blockId").is_none())
        .map(|r| json!({"id":r["id"],"title":r["title"],"status":r["status"],"kind":r["kind"]}))
        .collect();
    let active: Vec<_> = pips.iter().filter(|r| r["status"] != "cancelled").collect();
    task["subtaskProgress"] = json!({"total":active.len(),"done":active.iter().filter(|r| r["status"] == "done").count()});
    task["subtaskPips"] = json!(pips);
    task["workflowRows"] = json!(rows);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn step(id: &str, kind: &str) -> Value {
        json!({"id":id,"title":id,"kind":kind,"due":"","done":false,"items":[]})
    }
    fn fixture() -> Value {
        let mut flow = step("reply", "flow");
        flow["due"] = json!("2026-11-20");
        let mut agree = step("agree", "task");
        agree["items"] = json!([step("branch", "task")]);
        let mut reminder = step("remind", "reminder");
        reminder["offset"] = json!("one week before the deadline");
        flow["items"] = json!([agree, step("decline", "exit"), reminder]);
        json!({"subtasks":[step("sent","task"),flow,step("received","task")]})
    }
    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 2).unwrap()
    }
    fn status(task: &Value, id: &str, date: NaiveDate) -> String {
        rows(&task["subtasks"], date)
            .unwrap()
            .iter()
            .find(|n| n["id"] == id)
            .map(|n| s(n, "status").into())
            .unwrap_or("hidden".into())
    }
    #[test]
    fn alternatives_exits_and_branches_restore_without_erasing_history() {
        let mut task = fixture();
        validate(&task["subtasks"]).unwrap();
        assert_eq!(status(&task, "branch", day()), "hidden");
        toggle(&mut task, "received", day()).unwrap();
        toggle(&mut task, "decline", day()).unwrap();
        assert_eq!(status(&task, "received", day()), "cancelled");
        assert_eq!(status(&task, "agree", day()), "cancelled");
        assert_eq!(status(&task, "remind", day()), "cancelled");
        assert!(toggle(&mut task, "agree", day()).is_err());
        toggle(&mut task, "decline", day()).unwrap();
        assert_eq!(status(&task, "received", day()), "done");
        assert_eq!(status(&task, "remind", day()), "snoozed");
        toggle(&mut task, "agree", day()).unwrap();
        assert_eq!(status(&task, "branch", day()), "ready");
        toggle(&mut task, "branch", day()).unwrap();
        toggle(&mut task, "agree", day()).unwrap();
        assert_eq!(status(&task, "branch", day()), "hidden");
        toggle(&mut task, "agree", day()).unwrap();
        assert_eq!(status(&task, "branch", day()), "done");
    }
    #[test]
    fn reminders_do_not_resolve_flow_and_completed_reminders_survive_exit() {
        let mut task = fixture();
        assert_eq!(status(&task, "remind", day()), "snoozed");
        assert_eq!(
            status(
                &task,
                "remind",
                NaiveDate::from_ymd_opt(2026, 11, 13).unwrap()
            ),
            "ready"
        );
        toggle(&mut task, "remind", day()).unwrap();
        assert_eq!(status(&task, "reply", day()), "ready");
        assert_eq!(status(&task, "agree", day()), "ready");
        toggle(&mut task, "decline", day()).unwrap();
        assert_eq!(status(&task, "remind", day()), "done");
        assert_eq!(status(&task, "received", day()), "cancelled");
    }
    #[test]
    fn reminder_claims_are_scheduled_separately_and_cancelled_at_authority() {
        let mut fields = fixture();
        fields["title"] = json!("Synthetic reminder owner");
        fields["subtasks"][1]["due"] =
            json!((crate::model::today() + Duration::days(7)).to_string());
        let task = crate::model::mutate(&[], &json!({"action":"create","fields":fields})).unwrap();
        let claim = json!({"action":"claim","id":task["id"],"period":crate::model::today().to_string(),"subtask_id":"remind","device":"test"});
        let claimed = crate::model::mutate(&[task.clone()], &claim).unwrap();
        let key = format!(
            "subtask:remind:{}:{}",
            crate::model::today(),
            crate::model::today()
        );
        assert_eq!(claimed["notifications"][&key]["state"], "pending");
        assert_eq!(crate::model::mutate(&[claimed], &claim).unwrap(), false);
        let mut cancelled = task.clone();
        toggle(&mut cancelled, "decline", crate::model::today()).unwrap();
        assert_eq!(crate::model::mutate(&[cancelled], &claim).unwrap(), false);
        let mut future = task;
        future["subtasks"][1]["due"] =
            json!((crate::model::today() + Duration::days(8)).to_string());
        assert_eq!(crate::model::mutate(&[future], &claim).unwrap(), false);
    }

    #[test]
    fn templates_reset_every_branch_and_validate_structure() {
        let mut task = fixture();
        toggle(&mut task, "agree", day()).unwrap();
        toggle(&mut task, "branch", day()).unwrap();
        let copied = instantiate(&task["subtasks"]);
        validate(&copied).unwrap();
        assert_ne!(copied[0]["id"], task["subtasks"][0]["id"]);
        assert_eq!(copied[1]["items"][0]["done"], false);
        assert_eq!(copied[1]["items"][0]["items"][0]["done"], false);
        task["subtasks"][1]["items"][1]["done"] = json!(true);
        assert!(validate(&task["subtasks"]).is_err());
        assert_eq!(offset_days("2 weeks before").unwrap(), 14);
        assert!(offset_days("-1 days before").is_err());
        assert!(offset_days("one week before garbage").is_err());
        let mut invalid = fixture();
        invalid["subtasks"][1]["due"] = json!("");
        assert!(validate(&invalid["subtasks"]).is_err());
    }
    #[test]
    fn sequential_workflow_blocks_choices_reminders_and_later_steps_until_ready() {
        let mut task = fixture();
        task["workflow"] = json!(true);
        let state = |task: &Value, id: &str| {
            task_rows(task, day())
                .unwrap()
                .into_iter()
                .find(|r| r["id"] == id)
                .unwrap()["status"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        assert_eq!(state(&task, "sent"), "ready");
        for id in ["reply", "agree", "decline", "remind", "received"] {
            assert_eq!(state(&task, id), "blocked");
        }
        assert!(toggle(&mut task, "received", day()).is_err());
        assert!(toggle(&mut task, "decline", day()).is_err());
        toggle(&mut task, "sent", day()).unwrap();
        assert_eq!(state(&task, "agree"), "ready");
        assert_eq!(state(&task, "remind"), "snoozed");
        toggle(&mut task, "remind", day()).unwrap();
        assert_eq!(state(&task, "received"), "blocked");
        toggle(&mut task, "agree", day()).unwrap();
        assert_eq!(state(&task, "branch"), "ready");
        assert_eq!(state(&task, "received"), "blocked");
        toggle(&mut task, "branch", day()).unwrap();
        assert_eq!(state(&task, "received"), "ready");
        toggle(&mut task, "received", day()).unwrap();
        toggle(&mut task, "sent", day()).unwrap();
        assert_eq!(state(&task, "received"), "blocked");
        let blocked_rows = task_rows(&task, day()).unwrap();
        assert!(blocked_rows
            .iter()
            .filter(|r| r["blockId"] == "reply")
            .all(|r| r["blocked"] == true));
        toggle(&mut task, "sent", day()).unwrap();
        assert_eq!(state(&task, "received"), "done");
        decorate(&mut task, day()).unwrap();
        assert_eq!(task["subtaskProgress"], json!({"done":4,"total":4}));
        toggle(&mut task, "agree", day()).unwrap();
        toggle(&mut task, "decline", day()).unwrap();
        assert_eq!(state(&task, "received"), "cancelled");
        toggle(&mut task, "decline", day()).unwrap();
        assert_eq!(state(&task, "received"), "blocked");
    }
    #[test]
    fn templates_copy_workflow_and_blocked_reminders_cannot_be_claimed() {
        let mut fields = fixture();
        fields["title"] = json!("Workflow template");
        fields["kind"] = json!("template");
        fields["workflow"] = json!(true);
        fields["subtasks"][1]["due"] =
            json!((crate::model::today() + Duration::days(7)).to_string());
        let template =
            crate::model::mutate(&[], &json!({"action":"create","fields":fields})).unwrap();
        let mut task = crate::model::mutate(
            &[template.clone()],
            &json!({"action":"create","template_id":template["id"],"fields":{"title":"Guest"}}),
        )
        .unwrap();
        assert_eq!(task["workflow"], true);
        let reminder = task["subtasks"][1]["items"][2]["id"].clone();
        let claim = json!({"action":"claim","id":task["id"],"period":crate::model::today().to_string(),"subtask_id":reminder,"device":"test"});
        assert_eq!(
            crate::model::mutate(&[task.clone()], &claim).unwrap(),
            false
        );
        let sent = task["subtasks"][0]["id"].as_str().unwrap().to_owned();
        toggle(&mut task, &sent, crate::model::today()).unwrap();
        assert!(crate::model::mutate(&[task], &claim).unwrap().is_object());
    }
}
