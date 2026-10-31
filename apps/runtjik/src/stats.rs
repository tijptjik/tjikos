use crate::store::Store;
use anyhow::Result;
use chrono::{Datelike, Duration, Local, NaiveDate};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const METRICS: [(&str, &str); 5] = [
    ("open", "Launcher opens"),
    ("launch", "App launches"),
    ("switch", "Window switches"),
    ("close", "Window closes"),
    ("refresh", "Index refreshes"),
];

impl Store {
    pub fn statistics(&self) -> Result<Value> {
        // Calendar dates use the same local timezone as the UI, including DST.
        let mut query = self.0.prepare(
            "SELECT date(time,'unixepoch','localtime'),action,COUNT(*) FROM events
             WHERE success=1 AND action IN ('open','launch','switch','close','refresh')
             GROUP BY 1,2 ORDER BY 1",
        )?;
        let mut days = BTreeMap::new();
        for row in query.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })? {
            let (date, action, count) = row?;
            days.insert(
                (NaiveDate::parse_from_str(&date, "%Y-%m-%d")?, action),
                count,
            );
        }
        Ok(summarize(days, Local::now().date_naive()))
    }
}

fn summarize(days: BTreeMap<(NaiveDate, String), i64>, today: NaiveDate) -> Value {
    let first = days
        .keys()
        .map(|(date, _)| *date)
        .filter(|d| *d <= today)
        .min();
    let opened_since = days
        .keys()
        .filter(|(d, a)| a == "open" && *d <= today)
        .map(|(d, _)| *d)
        .min();
    let metrics: Vec<_> = METRICS.iter().map(|(action, label)| {
        let since = if *action == "open" { opened_since } else { first };
        let elapsed = since.map(|d| (today - d).num_days() + 1).unwrap_or(0);
        let total: i64 = days.iter().filter(|((d, a), _)| a == action && *d <= today).map(|(_, count)| count).sum();
        json!({"id":action,"label":label,"total":total,"average":if elapsed>0 {total as f64/elapsed as f64} else {0.0},"days":elapsed,"since":since.map(|d|d.to_string())})
    }).collect();
    // 26 complete week columns including the current, partial week. Monday first.
    let start = today - Duration::days(today.weekday().num_days_from_monday() as i64 + 25 * 7);
    let calendar: Vec<_> = (0..182).map(|offset| {
        let date = start + Duration::days(offset);
        json!({"date":date.to_string(),"count":days.get(&(date,"open".into())).copied().unwrap_or(0),"future":date>today,"tracked":opened_since.is_some_and(|d|date>=d) && date<=today})
    }).collect();
    json!({"metrics":metrics,"calendar":calendar,"today":today.to_string(),"since":first.map(|d|d.to_string()),"opened_since":opened_since.map(|d|d.to_string())})
}

pub fn demo() -> Value {
    let today = Local::now().date_naive();
    let mut days = BTreeMap::new();
    for offset in 0..180 {
        let date = today - Duration::days(offset);
        for (index, (action, _)) in METRICS.iter().enumerate() {
            let count = if offset % 11 == 3 || date.weekday().number_from_monday() == 7 {
                0
            } else {
                (offset * 13 + index as i64 * 7) % 27 + 1
            };
            days.insert(
                (date, (*action).into()),
                if index > 2 { count / 8 } else { count },
            );
        }
    }
    summarize(days, today)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_has_seven_day_columns_and_separate_open_tracking() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 31).unwrap();
        let days = BTreeMap::from([
            ((today - Duration::days(9), "launch".into()), 6),
            ((today - Duration::days(2), "open".into()), 3),
            ((today, "open".into()), 3),
            ((today, "switch".into()), 4),
        ]);
        let stats = summarize(days, today);
        assert_eq!(stats["metrics"][0]["total"], 6);
        assert_eq!(stats["metrics"][0]["average"], 2.0); // Includes the idle day.
        assert_eq!(stats["metrics"][1]["average"], 0.6);
        let calendar = stats["calendar"].as_array().unwrap();
        assert_eq!(calendar.len(), 26 * 7);
        assert_eq!(
            NaiveDate::parse_from_str(calendar[0]["date"].as_str().unwrap(), "%Y-%m-%d")
                .unwrap()
                .weekday()
                .num_days_from_monday(),
            0
        );
        assert_eq!(calendar[181]["future"], true);
        assert_eq!(calendar[0]["tracked"], false);
        assert_eq!(calendar[179]["tracked"], true);
        assert_eq!(calendar[179]["count"], 0);
    }

    #[test]
    fn database_excludes_failed_actions_and_keeps_calendar_days() {
        let store = Store(rusqlite::Connection::open_in_memory().unwrap());
        store
            .0
            .execute_batch(
                "CREATE TABLE events(time REAL,action TEXT,success INTEGER);
            INSERT INTO events VALUES(strftime('%s','now'),'open',1),
            (strftime('%s','now'),'open',0),(strftime('%s','now'),'launch',1),
            (strftime('%s','now'),'switch',0);",
            )
            .unwrap();
        let stats = store.statistics().unwrap();
        assert_eq!(stats["metrics"][0]["total"], 1);
        assert_eq!(stats["metrics"][1]["total"], 1);
        assert_eq!(stats["metrics"][2]["total"], 0);
        assert_eq!(stats["metrics"][0]["days"], 1);
    }

    #[test]
    fn empty_history_has_no_invented_tracking() {
        let stats = summarize(BTreeMap::new(), Local::now().date_naive());
        assert!(stats["opened_since"].is_null());
        assert_eq!(stats["metrics"][0]["average"], 0.0);
        assert!(stats["calendar"]
            .as_array()
            .unwrap()
            .iter()
            .all(|day| day["tracked"] == false));
    }
}
