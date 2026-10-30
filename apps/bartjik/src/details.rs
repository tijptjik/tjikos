use crate::util::*;
use anyhow::Result;
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
pub fn ini(raw: &str) -> Value {
    let mut out = json!({});
    let mut section = String::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].into();
            out[&section] = json!({});
        } else if let Some((k, v)) = line.split_once('=') {
            if !section.is_empty() {
                out[&section][k.trim()] = json!(v.trim().trim_matches('"'));
            }
        }
    }
    out
}
pub fn methods(group: &str) -> Vec<String> {
    let p = ini(
        &std::fs::read_to_string(xdg("XDG_CONFIG_HOME", ".config").join("fcitx5/profile"))
            .unwrap_or_default(),
    );
    let Some(prefix) = obj(&p)
        .into_iter()
        .find(|(k, v)| {
            k.starts_with("Groups/") && k.matches('/').count() == 1 && v["Name"] == group
        })
        .map(|(k, _)| k)
    else {
        return vec![];
    };
    let mut entries: Vec<_> = obj(&p)
        .into_iter()
        .filter(|(k, v)| k.starts_with(&format!("{prefix}/Items/")) && truth(&v["Name"]))
        .collect();
    entries.sort_by_key(|(k, _)| {
        k.rsplit('/')
            .next()
            .unwrap_or("")
            .parse::<usize>()
            .unwrap_or(usize::MAX)
    });
    entries.iter().map(|(_, v)| s(&v["Name"]).into()).collect()
}
pub fn world_clock(cities: &Value, timestamp: Option<i64>) -> Value {
    let defaults = json!([{"label":"New York","timezone":"America/New_York"},{"label":"London","timezone":"Europe/London"},{"label":"Amsterdam","timezone":"Europe/Amsterdam"},{"label":"Tokyo","timezone":"Asia/Tokyo"}]);
    let now = timestamp
        .and_then(|t| Utc.timestamp_opt(t, 0).single())
        .unwrap_or_else(Utc::now);
    let rows: Vec<_> = arr(if cities.is_null() { &defaults } else { cities })
        .iter()
        .filter_map(|c| {
            let tz = s(&c["timezone"]).parse::<chrono_tz::Tz>().ok()?;
            let local = now.with_timezone(&tz);
            Some(field(
                s(&c["label"]),
                json!(local.format("%H:%M").to_string()),
                &local.format("%a %d %b · %Z").to_string(),
            ))
        })
        .collect();
    panel(
        "World clock",
        json!(rows),
        "Local times · daylight saving aware",
    )
}
pub fn grouped(n: f64, digits: usize) -> String {
    let raw = format!("{n:.digits$}");
    let (whole, frac) = raw.split_once('.').unwrap_or((&raw, ""));
    let mut result = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 && c != '-' && whole.as_bytes()[i - 1] != b'-' {
            result.push(',');
        }
        result.push(c);
    }
    if digits > 0 {
        result.push('.');
        result.push_str(frac);
    }
    result
}
pub fn percent(v: &Value, digits: usize) -> String {
    if let Some(n) = v.as_f64() {
        let x = format!("{n:.digits$}");
        format!(
            "{}%",
            if digits > 0 {
                x.trim_end_matches('0').trim_end_matches('.')
            } else {
                &x
            }
        )
    } else {
        "-".into()
    }
}
pub fn reset(v: &Value) -> String {
    let Some(t) = v.as_i64() else {
        return "-".into();
    };
    let mins = ((t - now() as i64).max(0)) / 60;
    let days = mins / 1440;
    let hours = mins % 1440 / 60;
    if days > 0 {
        format!("{days}d{hours:02}h")
    } else if hours > 0 {
        format!("{hours}h{}m", mins % 60)
    } else {
        format!("{mins}m")
    }
}
pub fn quota(rows: &[Value], summary: Vec<Value>) -> Value {
    let mut accounts = std::collections::BTreeMap::new();
    for r in rows {
        accounts.insert(s(&r["label"]), r);
    }
    let (mut sum, mut weights) = (0., 0.);
    for r in accounts.values() {
        if let Some(v) = r["remaining_percent"].as_f64() {
            let weight = match s(&r["plan_type"]).to_lowercase().as_str() {
                "free" => 1134.,
                "plus" | "business" | "team" | "edu" => 7560.,
                "pro" | "enterprise" => 50400.,
                "prolite" => 37800.,
                _ => 1.,
            };
            sum += v * weight;
            weights += weight;
        }
    }
    let remaining = percent(
        &if weights > 0. {
            json!(sum / weights)
        } else {
            Value::Null
        },
        0,
    );
    let width = accounts
        .keys()
        .map(|s| s.chars().count())
        .max()
        .unwrap_or(5);
    let mut lines = vec![format!("{:<width$}  {:>5}  {:>4}", "acct", "1w%", "1w↻")];
    for (label, r) in &accounts {
        lines.push(format!(
            "{label:<width$}  {:>5}  {:>4}",
            percent(&r["remaining_percent"], 1),
            reset(&r["reset_at"])
        ));
    }
    let tooltip = if accounts.is_empty() {
        "No active Codex accounts".into()
    } else {
        lines.join("\n")
    };
    let mut result = item(
        "weekly",
        Some(&remaining),
        Some(&icon("codex")),
        "image",
        json!({}),
        &tooltip,
        json!({}),
    );
    let mut rows_out = vec![crate::config::merge(
        &field(
            "Weekly remaining",
            json!(remaining),
            "Capacity-weighted across active accounts",
        ),
        &json!({"id":"weekly"}),
    )];
    rows_out.extend(summary);
    for r in rows {
        rows_out.push(crate::config::merge(&field(s(&r["label"]),json!(percent(&r["remaining_percent"],1)),&format!("{} · resets in {}",s(&r["plan_type"]),reset(&r["reset_at"]))),&json!({"progress":r["remaining_percent"].as_f64().map(|n|(n/100.).clamp(0.,1.)).unwrap_or(-1.),"group":"account","plan":s(&r["plan_type"]),"resetAt":r["reset_at"]})));
    }
    result["panel"] = panel(
        "Codex usage",
        json!(rows_out),
        "codex-lb · read-only · weekly quota",
    );
    result
}
pub fn query_quota(path: &std::path::Path) -> Result<(Vec<Value>, Vec<Value>)> {
    let con =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    con.busy_timeout(std::time::Duration::from_secs(2))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
    con.progress_handler(10000, Some(move || std::time::Instant::now() > deadline));
    let mut q = con.prepare(include_str!("quota.sql"))?;
    let rows=q.query_map([],|r|Ok(json!({"label":r.get::<_,String>(1)?,"plan_type":r.get::<_,Option<String>>(2)?,"window_name":r.get::<_,String>(3)?,"remaining_percent":r.get::<_,Option<f64>>(4)?,"reset_at":r.get::<_,Option<i64>>(5)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let summary=con.query_row("SELECT COUNT(*), SUM(COALESCE(input_tokens,0)), SUM(COALESCE(output_tokens,0)), SUM(COALESCE(cached_input_tokens,0)), SUM(COALESCE(cost_usd,0)), AVG(latency_ms), SUM(CASE WHEN status IN ('error','failed','failure') THEN 1 ELSE 0 END) FROM request_logs WHERE requested_at >= datetime('now','-24 hours') AND deleted_at IS NULL",[],|r|{let mut ns=vec![];for i in 0..7{ns.push(r.get::<_,Option<f64>>(i)?.unwrap_or(0.));}Ok(ns)}).map(|n|vec![crate::config::merge(&field("Requests · 24h",json!(grouped(n[0],0)),&format!("{} failed",grouped(n[6],0))),&json!({"id":"requests","amount":n[0],"failures":n[6]})),crate::config::merge(&field("Input / output tokens",json!(format!("{} / {}",grouped(n[1],0),grouped(n[2],0))),""),&json!({"id":"tokens","input":n[1],"output":n[2]})),crate::config::merge(&field("Cached input",json!(grouped(n[3],0)),""),&json!({"id":"cached","amount":n[3]})),crate::config::merge(&field("Recorded cost · 24h",json!(format!("${}",grouped(n[4],2))),"Reported by codex-lb; not subscription billing"),&json!({"id":"cost","amount":n[4]})),crate::config::merge(&field("Mean request latency",json!(format!("{:.1}s",n[5]/1000.)),""),&json!({"id":"latency","seconds":n[5]/1000.}))]).unwrap_or_else(|_|vec![field("Request totals",json!("Unavailable"),"Database schema or query limit")]);
    Ok((rows, summary))
}
