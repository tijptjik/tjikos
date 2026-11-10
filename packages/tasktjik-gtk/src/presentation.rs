//! Calendar boundaries match Tasktjik's calendar views, including overdue tasks.
use chrono::{Datelike, Duration, NaiveDate};
pub fn horizon(today: &str, view: u32) -> String {
    let today = NaiveDate::parse_from_str(today, "%Y-%m-%d")
        .unwrap_or_else(|_| chrono::Local::now().date_naive());
    match view {
        2 => today + Duration::days(6 - i64::from(today.weekday().num_days_from_monday())),
        3 => {
            let (year, month) = if today.month() == 12 {
                (today.year() + 1, 1)
            } else {
                (today.year(), today.month() + 1)
            };
            NaiveDate::from_ymd_opt(year, month, 1).unwrap() - Duration::days(1)
        }
        _ => today,
    }
    .to_string()
}
#[cfg(test)]
mod tests {
    #[test]
    fn calendar_views_end_on_sunday_and_month_end() {
        assert_eq!(super::horizon("2026-11-10", 2), "2026-11-15");
        assert_eq!(super::horizon("2026-11-15", 2), "2026-11-15");
        assert_eq!(super::horizon("2024-02-12", 3), "2024-02-29");
        assert_eq!(super::horizon("2026-12-20", 3), "2026-12-31");
    }
}

/// Compact task/subtask date labels, matching the original DateCode.js contract.
pub fn date_code(value: &str, today: &str, view: u32) -> String {
    let (Ok(date), Ok(today)) = (
        NaiveDate::parse_from_str(value, "%Y-%m-%d"),
        NaiveDate::parse_from_str(today, "%Y-%m-%d"),
    ) else {
        return "·".into();
    };
    let delta = (date - today).num_days();
    let weekday = i64::from(today.weekday().num_days_from_monday());
    if view == 1 || view == 2 || (-weekday..=6 - weekday).contains(&delta) {
        return date.format("%a").to_string().to_uppercase();
    }
    let days = delta.abs();
    if days < 10 {
        return format!("{days}D");
    }
    let (start, end) = if delta < 0 {
        (date, today)
    } else {
        (today, date)
    };
    let months = (end.year() - start.year()) * 12 + end.month() as i32
        - start.month() as i32
        - i32::from(end.day() < start.day());
    if months < 3 {
        format!("{}W", (days / 7).max(1))
    } else if months < 12 {
        format!("{months}M")
    } else {
        format!("{}Y", months / 12)
    }
}
#[test]
fn compact_dates_retain_qml_labels() {
    for (date, view, expected) in [
        ("", 0, "·"),
        ("2026-11-06", 0, "FRI"),
        ("2026-11-13", 0, "1W"),
        ("2026-11-20", 3, "2W"),
        ("2026-11-20", 2, "FRI"),
        ("2026-10-31", 0, "2D"),
        ("2027-02-02", 0, "3M"),
        ("2027-11-02", 0, "1Y"),
    ] {
        assert_eq!(date_code(date, "2026-11-02", view), expected);
    }
}

/// The original project bar counts all non-cancelled children, independent of
/// the task visibility toggles. Calendar views count completions in that period.
pub fn project_progress(
    project: &serde_json::Value,
    state: &serde_json::Value,
    view: u32,
) -> (usize, usize) {
    use super::s;
    let today = NaiveDate::parse_from_str(s(state, "today"), "%Y-%m-%d")
        .unwrap_or_else(|_| chrono::Local::now().date_naive());
    let first = match view {
        2 => today - Duration::days(i64::from(today.weekday().num_days_from_monday())),
        3 => today.with_day(1).unwrap(),
        _ => today,
    }
    .to_string();
    let last = horizon(&today.to_string(), view);
    let within = |date: &str| {
        let date = date.get(..10).unwrap_or(date);
        !date.is_empty() && date >= first.as_str() && date <= last.as_str()
    };
    let mut total = 0;
    let mut done = 0;
    for task in state["tasks"].as_array().into_iter().flatten().filter(|t| {
        t["project"] == project["id"] && s(t, "kind") == "todo" && s(t, "status") != "cancelled"
    }) {
        let completed = if view == 0 {
            task["done"] == true
        } else {
            within(s(task, "completedAt"))
                || task["completions"]
                    .as_object()
                    .is_some_and(|dates| dates.values().any(|d| within(d.as_str().unwrap_or(""))))
        };
        let due = if s(task, "effectiveDue").is_empty() {
            s(task, "due")
        } else {
            s(task, "effectiveDue")
        };
        if view == 0 || within(due) || completed {
            total += 1;
            done += usize::from(completed);
        }
    }
    (done, total)
}

#[test]
fn project_progress_matches_calendar_and_cancelled_semantics() {
    use serde_json::json;
    let project = json!({"id":"project"});
    let state = json!({"today":"2026-11-10","tasks":[
        {"kind":"todo","project":"project","done":true,"completedAt":"2026-11-09T12:00:00Z"},
        {"kind":"todo","project":"project","done":false,"due":"2026-11-12"},
        {"kind":"todo","project":"project","done":false,"completions":{"period":"2026-11-10T12:00:00Z"}},
        {"kind":"todo","project":"project","done":true,"status":"cancelled"},
        {"kind":"todo","project":"other","done":true}
    ]});
    assert_eq!(project_progress(&project, &state, 0), (1, 3));
    assert_eq!(project_progress(&project, &state, 1), (1, 1));
    assert_eq!(project_progress(&project, &state, 2), (2, 3));
    assert_eq!(project_progress(&project, &state, 3), (2, 3));
    assert_eq!(project_progress(&json!({"id":"empty"}), &state, 0), (0, 0));
}

/// Latest completion, including recurring-task periods; missing dates sort last.
pub fn completion_time(task: &serde_json::Value) -> String {
    std::iter::once(super::s(task, "completedAt"))
        .chain(
            task["completions"]
                .as_object()
                .into_iter()
                .flat_map(|m| m.values())
                .filter_map(|v| v.as_str()),
        )
        .filter_map(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .max()
        .map(|d| d.with_timezone(&chrono::Utc).to_rfc3339())
        .unwrap_or_default()
}
#[test]
fn completion_sort_uses_latest_period_and_handles_timezones() {
    use serde_json::json;
    assert_eq!(completion_time(&json!({})), "");
    let a = json!({"completedAt":"2026-11-10T10:00:00+08:00","completions":{"once":"2026-11-10T03:00:00Z"}});
    let b = json!({"completedAt":"2026-11-10T02:30:00Z"});
    assert!(completion_time(&a) > completion_time(&b));
}
