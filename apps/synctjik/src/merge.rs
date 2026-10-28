use crate::util::*;
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
pub type Entries = BTreeMap<String, Value>;
pub fn entries(v: &Value) -> Result<Entries> {
    Ok(serde_json::from_value(v.clone())?)
}
pub fn merge(
    base: &Entries,
    local: &Entries,
    incoming: &Entries,
    leader: bool,
    resolutions: &BTreeMap<String, String>,
) -> Result<(Entries, Vec<Value>, Vec<String>)> {
    let mut desired = Entries::new();
    let mut changes = Vec::new();
    let mut conflicts = BTreeSet::new();
    for name in base
        .keys()
        .chain(local.keys())
        .chain(incoming.keys())
        .collect::<BTreeSet<_>>()
    {
        let (b, l, r) = (base.get(name), local.get(name), incoming.get(name));
        let choice = if let Some(d) = resolutions.get(name) {
            match d.as_str() {
                "local" => l,
                "source" => r,
                _ => bail!("Resolution must be local or source"),
            }
        } else if leader || l == r || l == b {
            r
        } else if r == b {
            l
        } else {
            conflicts.insert(name.clone());
            l
        };
        if let Some(c) = choice {
            desired.insert(name.clone(), c.clone());
        }
        if choice != l {
            changes.push(json!({"path":name,"action":if choice.is_none(){"delete"}else if l.is_none(){"add"}else{"replace"}}));
        }
    }
    for name in desired.keys().cloned().collect::<Vec<_>>() {
        let mut p = name.as_str();
        while let Some((parent, _)) = p.rsplit_once('/') {
            if !desired.contains_key(parent)
                && local.get(parent).is_some_and(|e| e["type"] == "dir")
            {
                desired.insert(parent.into(), local[parent].clone());
                changes.retain(|v| v["path"] != parent);
            }
            p = parent;
        }
    }
    for name in desired.keys() {
        let mut p = name.as_str();
        while let Some((parent, _)) = p.rsplit_once('/') {
            if !desired.get(parent).is_some_and(|e| e["type"] == "dir") {
                conflicts.insert(parent.into());
                conflicts.insert(name.clone());
            }
            p = parent;
        }
    }
    Ok((desired, changes, conflicts.into_iter().collect()))
}
pub fn managed(p: &Value) -> Value {
    let mut v = p.clone();
    for k in ["encrypted", "retention", "devices"] {
        v.as_object_mut().unwrap().remove(k);
    }
    v
}
pub fn backup_policy(p: &Value) -> Value {
    let mut v = p.clone();
    for k in ["devices", "retention", "review"] {
        v.as_object_mut().unwrap().remove(k);
    }
    v["encrypted"] = json!(encrypted(p));
    v
}
pub fn retention(
    refs: &[Value],
    latest: &str,
    policy: &str,
    current: chrono::DateTime<chrono::Utc>,
) -> Result<BTreeSet<String>> {
    use chrono::{Datelike, Duration};
    let mut keep = BTreeSet::from([latest.into()]);
    if policy == "latest" {
        return Ok(keep);
    }
    if policy != "daily-weekly-monthly" {
        bail!("Unknown retention policy")
    }
    let day = current.date_naive();
    let monday = day - Duration::days(day.weekday().num_days_from_monday() as i64);
    let month = day.with_day(1).unwrap();
    let prev = (month - Duration::days(1)).with_day(1).unwrap();
    let mut dated = Vec::new();
    for r in refs {
        dated.push((
            chrono::DateTime::parse_from_rfc3339(text(r, "created_at")?)?
                .with_timezone(&chrono::Utc),
            text(r, "snapshot")?.to_owned(),
        ));
    }
    dated.sort();
    for (start, end) in [(monday - Duration::days(7), monday), (prev, month)] {
        if let Some((_, id)) = dated
            .iter()
            .rev()
            .find(|(d, _)| d.date_naive() >= start && d.date_naive() < end)
        {
            keep.insert(id.clone());
        }
    }
    Ok(keep)
}
