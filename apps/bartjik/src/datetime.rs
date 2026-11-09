//! Shared datetime provider. Formatting transport and timezone policy stay out of widgets.
use crate::util::*;
use anyhow::{ensure, Result};
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub text: String,
    pub alternate: String,
    pub tooltip: String,
    pub panel: Value,
}
pub async fn read(module: &Value, timestamp: Option<i64>) -> Result<Snapshot> {
    let mut env = vec![];
    for (key, name) in [("timezone", "TZ"), ("locale", "LC_TIME")] {
        if let Some(value) = module[key].as_str() {
            env.push((name.into(), value.into()));
        } else if timestamp.is_some() {
            env.push((
                name.into(),
                if key == "timezone" { "UTC" } else { "C" }.into(),
            ));
        }
    }
    let mut args = vec!["date".into()];
    if let Some(timestamp) = timestamp {
        args.push(format!("--date=@{timestamp}"));
    }
    args.push(format!(
        "+{}\n{}\n{}",
        module["format"].as_str().unwrap_or("%H:%M"),
        module["alternate"].as_str().unwrap_or("%a %d %b"),
        module["tooltipFormat"].as_str().unwrap_or("%A, %d %B %Y")
    ));
    let raw = command(&args, 5., 262144, &env, None).await?;
    let values: Vec<_> = raw.lines().collect();
    ensure!(values.len() == 3, "date formats must produce one line each");
    Ok(Snapshot {
        text: values[0].into(),
        alternate: values[1].into(),
        tooltip: values[2].into(),
        panel: world_clock(&module["worldClocks"], timestamp),
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn configured_timezone_formats_and_literal_text() {
        let spec = json!({"timezone":"Asia/Hong_Kong","locale":"C","format":"%H:%M","alternate":"%Y-%m-%d","tooltipFormat":"<clock> %Z","worldClocks":[]});
        let value = read(&spec, Some(0)).await.unwrap();
        assert_eq!(value.text, "08:00");
        assert_eq!(value.alternate, "1970-01-01");
        assert_eq!(value.tooltip, "<clock> HKT");
        assert_eq!(value.panel["rows"], json!([]));
        let mut invalid = spec;
        invalid["format"] = json!("a\nb");
        assert!(read(&invalid, Some(0)).await.is_err());
    }
    #[test]
    fn daylight_saving_and_date_boundary() {
        let cities = json!([{"label":"New York","timezone":"America/New_York"},{"label":"Tokyo","timezone":"Asia/Tokyo"}]);
        let before = Utc
            .with_ymd_and_hms(2026, 3, 8, 6, 59, 0)
            .unwrap()
            .timestamp();
        let after = before + 60;
        assert_eq!(
            world_clock(&cities, Some(before))["rows"][0]["value"],
            "01:59"
        );
        assert_eq!(
            world_clock(&cities, Some(after))["rows"][0]["value"],
            "03:00"
        );
        let midnight = Utc
            .with_ymd_and_hms(2026, 3, 8, 16, 0, 0)
            .unwrap()
            .timestamp();
        assert_eq!(
            world_clock(&cities, Some(midnight))["rows"][1]["value"],
            "01:00"
        );
        assert!(world_clock(&cities, Some(midnight))["rows"][1]["detail"]
            .as_str()
            .unwrap()
            .contains("Mon 09 Mar"));
    }
}
