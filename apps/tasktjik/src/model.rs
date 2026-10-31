use anyhow::{bail, ensure, Context, Result};
use chrono::{Datelike, Duration, Local, Months, NaiveDate, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn now() -> String {
    Utc::now().to_rfc3339()
}
pub fn today() -> NaiveDate {
    Local::now().date_naive()
}
fn date(s: &str) -> Result<NaiveDate> {
    Ok(NaiveDate::parse_from_str(s, "%Y-%m-%d")?)
}
fn field<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn interval(v: &Value) -> Result<i64> {
    let n = match v.as_i64() {
        Some(n) => n,
        None => v.as_str().unwrap_or("").parse()?,
    };
    ensure!(
        (1..=10000).contains(&n),
        "Repeat interval must be between 1 and 10000"
    );
    Ok(n)
}
fn days(v: &Value, max: u32) -> Result<Vec<u32>> {
    let text = v
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v.to_string());
    let result: Vec<u32> = text
        .split(',')
        .map(|s| s.trim().parse())
        .collect::<std::result::Result<_, _>>()?;
    ensure!(
        !result.is_empty() && result.iter().all(|n| (1..=max).contains(n)),
        "Invalid repeat days"
    );
    Ok(result)
}
pub fn period(task: &Value, day: NaiveDate) -> Result<String> {
    let due = field(task, "due");
    if due.is_empty() {
        return Ok("once".into());
    }
    let start = date(due)?;
    let unit = field(&task["repeat"], "unit");
    let value = &task["repeat"]["value"];
    if unit.is_empty() {
        return Ok(due.into());
    }
    if unit.starts_with("days-of-") {
        let week = unit == "days-of-the-week";
        let allowed = days(value, if week { 7 } else { 31 })?;
        let matches = |d: NaiveDate| {
            allowed.contains(&if week {
                d.weekday().number_from_monday()
            } else {
                d.day()
            })
        };
        let mut cursor = day;
        while cursor >= start {
            if matches(cursor) {
                return Ok(cursor.to_string());
            }
            cursor -= Duration::days(1);
        }
        cursor = start;
        while !matches(cursor) {
            cursor += Duration::days(1);
        }
        return Ok(cursor.to_string());
    }
    if day < start {
        return Ok(due.into());
    }
    let value = interval(value)?;
    if unit == "days" || unit == "weeks" {
        let span = value * if unit == "weeks" { 7 } else { 1 };
        return Ok((start + Duration::days((day - start).num_days() / span * span)).to_string());
    }
    let span = value as u32 * if unit == "years" { 12 } else { 1 };
    let elapsed =
        ((day.year() - start.year()) * 12 + day.month() as i32 - start.month() as i32) as u32;
    let mut n = elapsed / span;
    let mut target = start
        .checked_add_months(Months::new(n * span))
        .context("Repeat date out of range")?;
    if target > day {
        n = n.saturating_sub(1);
        target = start
            .checked_add_months(Months::new(n * span))
            .context("Repeat date out of range")?;
    }
    Ok(target.to_string())
}
pub fn defaults(t: &mut Value) {
    if !t["status"].is_string() {
        t["status"] = json!(if t["completedAt"].is_string() {
            "done"
        } else {
            "ready"
        });
    }
    for (k, v) in [
        ("source", json!({})),
        ("assignee", json!("")),
        ("project", json!("")),
        ("blockers", json!([])),
        ("acceptance", json!([])),
        ("verification", json!([])),
        ("events", json!([])),
        ("kind", json!("todo")),
        (
            "reference",
            t["plan"].as_str().map(|s| json!(s)).unwrap_or(json!("")),
        ),
    ] {
        if t.get(k).is_none() {
            t[k] = v;
        }
    }
}
pub fn validate(t: &mut Value) -> Result<()> {
    ensure!(t.is_object(), "Task must be an object");
    for key in ["due", "owner", "createdAt", "updatedAt"] {
        ensure!(t[key].is_string(), "Invalid {key}");
    }
    for key in ["completions", "notifications"] {
        ensure!(t[key].is_object(), "Invalid {key}");
    }
    for key in ["events", "verification", "blockers"] {
        ensure!(t[key].is_array(), "Invalid {key}");
    }
    ensure!(
        t["revision"].as_u64().is_some_and(|n| n < u64::MAX),
        "Invalid revision"
    );
    if let Some(until) = t["snoozedUntil"].as_str() {
        chrono::DateTime::parse_from_rfc3339(until)?;
    }
    let title = field(t, "title").trim().to_owned();
    ensure!(
        !title.is_empty() && title.len() <= 4096,
        "Title is required and must be at most 4096 bytes"
    );
    t["title"] = json!(title);
    let group = field(t, "group").trim().to_owned();
    let group = if group.is_empty() {
        "All".to_owned()
    } else {
        group
    };
    ensure!(
        group.chars().count() <= 100 && !group.chars().any(char::is_control),
        "Invalid group"
    );
    t["group"] = json!(group);
    ensure!(
        ["todo", "reading"].contains(&field(t, "kind")),
        "Invalid task kind"
    );
    for key in ["reference", "plan", "assignee", "project"] {
        ensure!(
            t[key].is_string() && field(t, key).len() <= 16384,
            "Invalid {key}"
        );
    }
    if field(t, "kind") == "reading" {
        let r = field(t, "reference");
        let authority = r
            .strip_prefix("https://")
            .or_else(|| r.strip_prefix("http://"))
            .context("Reading reference must be HTTP(S)")?;
        ensure!(
            !authority.split('/').next().unwrap_or("").is_empty()
                && !r.chars().any(char::is_control),
            "Invalid reading URL"
        );
    }
    ensure!(t["source"].is_object(), "Invalid source");
    for k in ["tags", "acceptance"] {
        ensure!(
            t[k].as_array().is_some_and(|a| a.len() <= 200
                && a.iter()
                    .all(|v| v.as_str().is_some_and(|s| s.len() <= 4096))),
            "Invalid {k}"
        );
    }
    if !field(t, "due").is_empty() {
        date(field(t, "due"))?;
    }
    let unit = field(&t["repeat"], "unit");
    ensure!(t["repeat"].is_object(), "Invalid repeat");
    if !unit.is_empty() {
        ensure!(
            !field(t, "due").is_empty(),
            "Repeating tasks need a due date"
        );
        ensure!(
            [
                "days",
                "weeks",
                "months",
                "years",
                "days-of-the-week",
                "days-of-the-month"
            ]
            .contains(&unit),
            "Invalid repeat unit"
        );
        if unit.starts_with("days-of-") {
            days(
                &t["repeat"]["value"],
                if unit == "days-of-the-week" { 7 } else { 31 },
            )?;
        } else {
            interval(&t["repeat"]["value"])?;
        }
    }
    ensure!(
        t["notifyBeforeDue"].is_boolean(),
        "Invalid notification setting"
    );
    ensure!(
        ["hours", "days", "weeks"].contains(&field(t, "notifyBeforeDueLeadUnit")),
        "Invalid notification lead unit"
    );
    ensure!(
        t["notifyBeforeDueLeadValue"]
            .as_i64()
            .is_some_and(|n| (0..=10000).contains(&n)),
        "Invalid notification lead value"
    );
    Ok(())
}
pub fn mutate(tasks: &[Value], r: &Value) -> Result<Value> {
    let op = field(r, "action");
    let mut t = if op == "create" {
        json!({"id":Uuid::new_v4().to_string(),"title":"","kind":"todo","reference":"","createKey":r["request_id"],"group":"All","tags":[],"due":"","repeat":{},"plan":"","owner":"tijptjik","notifyBeforeDue":true,"notifyBeforeDueLeadValue":1,"notifyBeforeDueLeadUnit":"days","snoozedUntil":null,"createdAt":now(),"updatedAt":now(),"completedAt":null,"completions":{},"notifications":{},"revision":0})
    } else {
        tasks
            .iter()
            .find(|t| t["id"] == r["id"])
            .context("Task no longer exists")?
            .clone()
    };
    defaults(&mut t);
    let period = if let Some(p) = r["period"].as_str() {
        if p != "once" {
            date(p)?;
        }
        p.to_owned()
    } else {
        period(&t, today())?
    };
    if ["edit", "status", "verify", "toggle", "purge"].contains(&op) {
        ensure!(
            r["revision"] == t["revision"],
            "Task changed; refresh before updating"
        );
    }
    if op == "purge" {
        return Ok(json!({"id":t["id"],"deleted":true}));
    }
    match op {
        "create" | "edit" => {
            let fields = r["fields"]
                .as_object()
                .context("fields must be an object")?;
            for k in [
                "title",
                "kind",
                "reference",
                "group",
                "tags",
                "due",
                "repeat",
                "plan",
                "notifyBeforeDue",
                "notifyBeforeDueLeadValue",
                "notifyBeforeDueLeadUnit",
                "source",
                "assignee",
                "project",
                "acceptance",
            ] {
                if let Some(v) = fields.get(k) {
                    t[k] = v.clone();
                }
            }
            if fields.contains_key("plan") && !fields.contains_key("reference") {
                t["reference"] = t["plan"].clone();
            }
            t["plan"] = t["reference"].clone();
            if field(&t, "group") == "Quick" {
                t["due"] = json!(today().to_string());
            }
            validate(&mut t)?;
        }
        "done" | "toggle" | "status" => {
            let done = if op == "status" {
                let status = field(r, "status");
                ensure!(
                    [
                        "inbox",
                        "ready",
                        "in_progress",
                        "blocked",
                        "done",
                        "cancelled"
                    ]
                    .contains(&status),
                    "Invalid task status"
                );
                let empty = json!([]);
                let blockers = r.get("blockers").unwrap_or(&empty);
                let list = blockers.as_array().context("Invalid blockers")?;
                ensure!(
                    status != "blocked" || !list.is_empty(),
                    "Blocked tasks need a reason"
                );
                for b in list {
                    ensure!(!field(b, "reason").is_empty(), "Blockers need a reason");
                    if let Some(id) = b["task_id"].as_str() {
                        ensure!(
                            id != field(&t, "id") && tasks.iter().any(|x| field(x, "id") == id),
                            "Invalid blocker task"
                        );
                    }
                }
                t["blockers"] = blockers.clone();
                t["status"] = json!(status);
                status == "done"
            } else {
                op == "done" || t["completions"].get(&period).is_none()
            };
            if done {
                if t["completions"].get(&period).is_none() {
                    t["completions"][&period] = json!(now());
                }
            } else {
                t["completions"]
                    .as_object_mut()
                    .context("Invalid completions")?
                    .remove(&period);
            }
            t["completedAt"] = t["completions"]
                .as_object()
                .unwrap()
                .values()
                .filter_map(Value::as_str)
                .max()
                .map(|s| json!(s))
                .unwrap_or(Value::Null);
            if op != "status" {
                t["status"] = json!(if done { "done" } else { "ready" });
            }
        }
        "verify" => {
            ensure!(
                ["passed", "failed", "pending"].contains(&field(r, "outcome")),
                "Invalid verification outcome"
            );
            ensure!(
                !field(r, "evidence").trim().is_empty() && !field(r, "artifact").trim().is_empty(),
                "Verification needs evidence and artifact"
            );
            let revision = t["revision"].clone();
            t["verification"].as_array_mut().context("Invalid verification history")?.push(json!({"actor":r["actor"],"at":now(),"outcome":r["outcome"],"evidence":r["evidence"],"artifact":r["artifact"],"taskRevision":revision,"period":period}));
        }
        "snooze" => {
            let until = chrono::DateTime::parse_from_rfc3339(field(r, "until"))?;
            ensure!(until > Utc::now(), "Snooze must be in the future");
            t["snoozedUntil"] = json!(until.to_rfc3339());
            t["notifications"]
                .as_object_mut()
                .unwrap()
                .remove(&format!("{period}:{}", today()));
        }
        "claim" | "ack" | "release" | "dismiss" => {
            let key = if ["ack", "release"].contains(&op) {
                r["notification_key"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{period}:{}", today()))
            } else {
                format!("{period}:{}", today())
            };
            let previous = t["notifications"].get(&key).cloned();
            if op == "claim" {
                if t["completions"].get(&period).is_some() || field(&t, "status") == "cancelled" {
                    return Ok(Value::Bool(false));
                }
                if let Some(p) = previous {
                    if field(&p, "state") != "pending"
                        || chrono::DateTime::parse_from_rfc3339(field(&p, "expires"))? > Utc::now()
                    {
                        return Ok(Value::Bool(false));
                    }
                }
                if let Some(until) = t["snoozedUntil"].as_str() {
                    if chrono::DateTime::parse_from_rfc3339(until)? > Utc::now() {
                        return Ok(Value::Bool(false));
                    }
                }
                t["notifications"][&key] = json!({"state":"pending","at":now(),"device":r["device"],"token":Uuid::new_v4().to_string(),"expires":(Utc::now()+Duration::minutes(2)).to_rfc3339()});
            } else if op == "ack" || op == "release" {
                let p = previous.context("Notification claim no longer exists")?;
                ensure!(
                    p["token"] == r["token"] && p["device"] == r["device"],
                    "Notification claim belongs to another device"
                );
                if op == "ack" {
                    t["notifications"][&key]["state"] = json!("delivered");
                } else if field(&p, "state") == "pending" {
                    t["notifications"].as_object_mut().unwrap().remove(&key);
                }
            } else {
                t["notifications"][&key] =
                    json!({"state":"dismiss","at":now(),"device":r["device"]});
            }
        }
        _ => bail!("Unknown operation"),
    }
    t["revision"] = json!(t["revision"].as_u64().context("Invalid revision")? + 1);
    t["updatedAt"] = json!(now());
    let revision = t["revision"].clone();
    let status = t["status"].clone();
    t["events"].as_array_mut().context("Invalid event history")?.push(json!({"action":op,"actor":r["actor"],"at":now(),"revision":revision,"status":status,"period":period}));
    Ok(t)
}

/// Next anchored occurrence, preserving month ends rather than drifting.
pub fn next_period(task: &Value, current: &str) -> Result<String> {
    let current = date(current)?;
    let rule = &task["repeat"];
    let unit = field(rule, "unit");
    if unit.starts_with("days-of-") {
        let week = unit == "days-of-the-week";
        let allowed = days(&rule["value"], if week { 7 } else { 31 })?;
        let mut day = current + Duration::days(1);
        while !allowed.contains(&if week {
            day.weekday().number_from_monday()
        } else {
            day.day()
        }) {
            day += Duration::days(1);
        }
        return Ok(day.to_string());
    }
    let value = interval(&rule["value"])?;
    if unit == "days" || unit == "weeks" {
        return Ok(
            (current + Duration::days(value * if unit == "weeks" { 7 } else { 1 })).to_string(),
        );
    }
    ensure!(["months", "years"].contains(&unit), "Invalid repeat unit");
    let anchor = date(field(task, "due"))?;
    let months =
        (current.year() - anchor.year()) * 12 + current.month() as i32 - anchor.month() as i32;
    ensure!(months >= 0, "Period precedes anchor");
    Ok(anchor
        .checked_add_months(Months::new(
            months as u32 + value as u32 * if unit == "years" { 12 } else { 1 },
        ))
        .context("Repeat date out of range")?
        .to_string())
}

#[cfg(test)]
mod recurrence_tests {
    use super::*;
    #[test]
    fn next_period_keeps_anchor_and_selected_days() {
        let task = json!({"due":"2024-01-31","repeat":{"unit":"months","value":1}});
        assert_eq!(next_period(&task, "2024-02-29").unwrap(), "2024-03-31");
        let task = json!({"due":"2024-02-29","repeat":{"unit":"years","value":1}});
        assert_eq!(next_period(&task, "2025-02-28").unwrap(), "2026-02-28");
        let task = json!({"due":"2024-01-01","repeat":{"unit":"days-of-the-month","value":"31"}});
        assert_eq!(next_period(&task, "2024-01-31").unwrap(), "2024-03-31");
    }
}
