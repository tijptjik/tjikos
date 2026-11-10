//! Calendar-only input shared by standalone and embedded editors.
use chrono::{Datelike, Duration, Months, NaiveDate};
use serde_json::{json, Value};
const WEEK: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];
const MONTH: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
fn words(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn number(text: &str) -> Option<u32> {
    if text == "a" || text == "an" {
        return Some(1);
    }
    [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "eleven", "twelve",
    ]
    .iter()
    .position(|v| *v == text)
    .map(|n| n as u32)
    .or_else(|| {
        (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
            .then(|| text.parse().ok())
            .flatten()
    })
}
fn ordinal(text: &str) -> &str {
    ["st", "nd", "rd", "th"]
        .iter()
        .find_map(|suffix| text.strip_suffix(suffix))
        .unwrap_or(text)
}
fn named(text: &str, names: &[&str]) -> Option<usize> {
    names.iter().position(|n| *n == text || &n[..3] == text)
}
fn shifted(today: NaiveDate, amount: u32, unit: &str, ago: bool) -> Option<NaiveDate> {
    if amount > 10000 {
        return None;
    }
    match unit.trim_end_matches('s') {
        "day" | "week" => today.checked_add_signed(Duration::days(
            i64::from(amount)
                * if unit.starts_with("week") { 7 } else { 1 }
                * if ago { -1 } else { 1 },
        )),
        "month" | "year" => {
            let months = Months::new(amount * if unit.starts_with("year") { 12 } else { 1 });
            if ago {
                today.checked_sub_months(months)
            } else {
                today.checked_add_months(months)
            }
        }
        _ => None,
    }
}
pub fn date(text: &str, today: NaiveDate) -> Result<String, String> {
    let text = words(text);
    if ["", "none", "no due date"].contains(&text.as_str()) {
        return Ok(String::new());
    }
    let result = || -> Option<NaiveDate> {
        if text.len() == 10 && text.as_bytes().get(4) == Some(&b'-') {
            return NaiveDate::parse_from_str(&text, "%Y-%m-%d").ok();
        }
        match text.as_str() {
            "today" => return Some(today),
            "tomorrow" => return today.succ_opt(),
            "yesterday" => return today.pred_opt(),
            "day after tomorrow" => return today.checked_add_signed(Duration::days(2)),
            "end of month" => {
                return today
                    .with_day(1)?
                    .checked_add_months(Months::new(1))?
                    .pred_opt()
            }
            _ => {}
        }
        let bare = text
            .strip_prefix("on ")
            .or_else(|| text.strip_prefix("this "))
            .or_else(|| text.strip_prefix("next "))
            .unwrap_or(&text);
        if let Some(day) = named(bare, &WEEK) {
            let mut delta =
                (day as i64 - i64::from(today.weekday().num_days_from_monday())).rem_euclid(7);
            if delta == 0 && text.starts_with("next ") {
                delta = 7
            }
            return today.checked_add_signed(Duration::days(delta));
        }
        if let Some(unit) = text.strip_prefix("next ") {
            return shifted(today, 1, unit, false);
        }
        let relative = text.strip_prefix("in ").unwrap_or(&text);
        let ago = relative.ends_with(" ago");
        let relative = relative
            .strip_suffix(" from now")
            .or_else(|| relative.strip_suffix(" ago"))
            .unwrap_or(relative);
        let parts: Vec<_> = relative.split_whitespace().collect();
        if parts.len() == 2 {
            if let Some(n) = number(parts[0]) {
                if let Some(d) = shifted(today, n, parts[1], ago) {
                    return Some(d);
                }
            }
        }
        let clean = bare.replace(',', "");
        let parts: Vec<_> = clean.split_whitespace().collect();
        if !(2..=3).contains(&parts.len()) {
            return None;
        }
        let (month, day) = if let Some(m) = named(parts[0], &MONTH) {
            (m + 1, number(ordinal(parts[1]))?)
        } else {
            (named(parts[1], &MONTH)? + 1, number(ordinal(parts[0]))?)
        };
        if parts.len() == 3 {
            return NaiveDate::from_ymd_opt(parts[2].parse().ok()?, month as u32, day);
        }
        (today.year()..=today.year() + 8)
            .filter_map(|y| NaiveDate::from_ymd_opt(y, month as u32, day))
            .find(|d| *d >= today)
    }();
    result
        .filter(|d| (1..=9999).contains(&d.year()))
        .map(|d| d.to_string())
        .ok_or_else(|| "Date not understood. Try “in two weeks” or “15 December”.".into())
}
pub fn repeat(text: &str) -> Result<Value, String> {
    let text = words(text);
    let simple = match text.as_str() {
        "" | "none" | "does not repeat" => return Ok(json!({})),
        "daily" => Some(("days", 1)),
        "weekly" => Some(("weeks", 1)),
        "fortnightly" => Some(("weeks", 2)),
        "monthly" => Some(("months", 1)),
        "yearly" | "annually" => Some(("years", 1)),
        _ => None,
    };
    if let Some((unit, value)) = simple {
        return Ok(json!({"unit":unit,"value":value}));
    }
    if ["every weekday", "weekdays"].contains(&text.as_str()) {
        return Ok(json!({"unit":"days-of-the-week","value":"1,2,3,4,5"}));
    }
    if ["every weekend", "weekends"].contains(&text.as_str()) {
        return Ok(json!({"unit":"days-of-the-week","value":"6,7"}));
    }
    let bare = text.strip_prefix("every ").unwrap_or(&text);
    let parts: Vec<_> = bare.split_whitespace().collect();
    let pair = match parts.as_slice() {
        [unit] => Some((1, *unit)),
        [n, unit] => number(n)
            .or_else(|| (*n == "other").then_some(2))
            .map(|n| (n, *unit)),
        _ => None,
    };
    if let Some((n, unit)) = pair {
        let unit = unit.trim_end_matches('s');
        if (1..=10000).contains(&n) && ["day", "week", "month", "year"].contains(&unit) {
            return Ok(json!({"unit":format!("{unit}s"),"value":n}));
        }
    }
    let monthly = text.strip_prefix("every month on the ").or_else(|| {
        text.strip_suffix(" of every month")
            .map(|s| s.strip_prefix("on the ").unwrap_or(s))
    });
    let source = monthly.unwrap_or_else(|| {
        text.strip_prefix("every ")
            .or_else(|| text.strip_prefix("on "))
            .unwrap_or(&text)
            .strip_suffix(" every week")
            .unwrap_or(
                text.strip_prefix("every ")
                    .or_else(|| text.strip_prefix("on "))
                    .unwrap_or(&text),
            )
    });
    let clean = source.replace(',', " ").replace(" and ", " ");
    let values: Option<Vec<u32>> = clean
        .split_whitespace()
        .map(|s| {
            if monthly.is_some() {
                number(ordinal(s)).filter(|n| (1..=31).contains(n))
            } else {
                named(s, &WEEK).map(|n| n as u32 + 1)
            }
        })
        .collect();
    if let Some(mut values) = values.filter(|v| !v.is_empty()) {
        values.sort();
        values.dedup();
        return Ok(
            json!({"unit":if monthly.is_some(){"days-of-the-month"}else{"days-of-the-week"},"value":values.iter().map(u32::to_string).collect::<Vec<_>>().join(",")}),
        );
    }
    Err("Repeat not understood. Try “every two weeks” or “every Monday and Friday”.".into())
}
pub fn repeat_label(value: &Value) -> String {
    let unit = value["unit"].as_str().unwrap_or("");
    if unit.is_empty() {
        return String::new();
    }
    let n = value["value"].as_u64().unwrap_or(1);
    if ["days", "weeks", "months", "years"].contains(&unit) {
        return format!("every {n} {unit}");
    }
    let values = value["value"].as_str().unwrap_or("").split(',');
    if unit == "days-of-the-week" {
        return format!(
            "every {}",
            values
                .filter_map(|v| v
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| n.checked_sub(1))
                    .and_then(|n| WEEK.get(n)))
                .copied()
                .collect::<Vec<_>>()
                .join(" and ")
        );
    }
    if unit == "days-of-the-month" {
        return format!(
            "on the {} of every month",
            values.collect::<Vec<_>>().join(" and ")
        );
    }
    unit.into()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates_keep_original_calendar_semantics() {
        let anchor = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        for (input, expected) in [
            ("", ""),
            ("Today", "2026-11-01"),
            ("next Sunday", "2026-11-08"),
            ("this Sunday", "2026-11-01"),
            ("in two weeks", "2026-11-15"),
            ("two days ago", "2026-10-30"),
            ("15 December", "2026-12-15"),
            ("Dec 15th, 2027", "2027-12-15"),
            ("Feb 29", "2028-02-29"),
            ("end of month", "2026-11-30"),
        ] {
            assert_eq!(date(input, anchor).unwrap(), expected, "{input}");
        }
        for input in [
            "2026-02-29",
            "February 30",
            "03/04/2026",
            "tomorrow at 5pm",
            "in -2 days",
            "in 1.5 weeks",
            "0000-01-01",
        ] {
            assert!(date(input, anchor).is_err(), "{input}");
        }
        assert_eq!(
            date("next month", NaiveDate::from_ymd_opt(2024, 1, 31).unwrap()).unwrap(),
            "2024-02-29"
        );
    }
    #[test]
    fn repeat_formats_round_trip_original_rules() {
        for input in [
            "daily",
            "every other day",
            "every two weeks",
            "fortnightly",
            "monthly",
            "annually",
            "every weekday",
            "every weekend",
            "every Monday and Friday",
            "on tue, thu",
            "Sunday every week",
            "on the 1st and 15th of every month",
            "every month on the 31st",
        ] {
            let rule = repeat(input).unwrap();
            assert_eq!(repeat(&repeat_label(&rule)).unwrap(), rule, "{input}");
        }
        for input in [
            "every 0 days",
            "every -1 weeks",
            "every 10001 years",
            "every 1.5 months",
            "sometimes",
            "every first Monday",
            "32nd of every month",
            "every Monday afternoon",
            "every month on the 0th",
        ] {
            assert!(repeat(input).is_err(), "{input}");
        }
    }
}
