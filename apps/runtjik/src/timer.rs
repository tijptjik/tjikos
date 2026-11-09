use crate::{
    discovery::Entry,
    store::{Store, TimerRecord},
    util::{home, now, run, s, which},
};
use anyhow::{Context, Result};
use chrono::{Local, TimeZone};
use regex::Regex;
use serde_json::json;
use std::{env, process::Command, sync::OnceLock};
use uuid::Uuid;

const MAX_SECONDS: u64 = 365 * 24 * 60 * 60;

fn duration_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)(\d+)\s*(seconds?|secs?|s|minutes?|mins?|m|hours?|hrs?|h|days?|d)")
            .unwrap()
    })
}

fn number_words(value: &str) -> String {
    let mut value = value.to_lowercase();
    for (from, to) in [
        ("half an hour", "30 minutes"),
        ("half hour", "30 minutes"),
        ("a quarter hour", "15 minutes"),
        ("quarter hour", "15 minutes"),
        ("an hour", "1 hour"),
        ("a minute", "1 minute"),
        ("a second", "1 second"),
    ] {
        value = value.replace(from, to);
    }
    let words = [
        ("zero", "0"),
        ("one", "1"),
        ("two", "2"),
        ("three", "3"),
        ("four", "4"),
        ("five", "5"),
        ("six", "6"),
        ("seven", "7"),
        ("eight", "8"),
        ("nine", "9"),
        ("ten", "10"),
        ("eleven", "11"),
        ("twelve", "12"),
    ];
    value
        .split_whitespace()
        .map(|word| {
            let clean = word.trim_matches(|c: char| !c.is_alphanumeric());
            words
                .iter()
                .find(|(name, _)| clean == *name)
                .map(|(_, number)| word.replace(clean, number))
                .unwrap_or_else(|| word.to_owned())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn duration_seconds(value: &str) -> Option<u64> {
    let mut input = number_words(value.trim());
    for prefix in ["in "] {
        if let Some(rest) = input.strip_prefix(prefix) {
            input = rest.to_owned();
        }
    }
    if let Some(rest) = input.strip_suffix(" from now") {
        input = rest.to_owned();
    }
    let mut seconds = 0_u64;
    let mut found = false;
    for capture in duration_regex().captures_iter(&input) {
        let amount = capture.get(1)?.as_str().parse::<u64>().ok()?;
        let unit = capture.get(2)?.as_str().to_lowercase();
        let factor = match unit.as_str() {
            "s" | "sec" | "secs" | "second" | "seconds" => 1,
            "m" | "min" | "mins" | "minute" | "minutes" => 60,
            "h" | "hr" | "hrs" | "hour" | "hours" => 60 * 60,
            "d" | "day" | "days" => 24 * 60 * 60,
            _ => return None,
        };
        seconds = seconds.checked_add(amount.checked_mul(factor)?)?;
        found = true;
    }
    let remainder = duration_regex().replace_all(&input, " ");
    let remainder = remainder
        .split_whitespace()
        .filter(|word| !["and", "in", "from", "now"].contains(word))
        .collect::<Vec<_>>();
    if !found || !remainder.is_empty() || seconds == 0 || seconds > MAX_SECONDS {
        return None;
    }
    Some(seconds)
}

fn date_seconds(value: &str) -> Option<i64> {
    let date = which("date")?;
    let value = value.replace(" at ", " ");
    let output = Command::new(date)
        .arg(format!("--date={value}"))
        .arg("+%s")
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

fn parse_spec(value: &str) -> Option<(u64, String)> {
    let value = value.trim();
    let lowered = number_words(value);
    if let Some(seconds) = duration_seconds(&lowered) {
        return Some((seconds, format!("{} timer", duration_label(seconds))));
    }

    if let Some((label, duration)) = lowered.rsplit_once(" in ") {
        if !label.trim().is_empty() {
            if let Some(seconds) = duration_seconds(duration) {
                return Some((seconds, label.trim().to_owned()));
            }
        }
    }

    let due_at = date_seconds(&lowered)?;
    let seconds = due_at.checked_sub(now().floor() as i64)?;
    if seconds <= 0 || seconds as u64 > MAX_SECONDS {
        return None;
    }
    Some((seconds as u64, value.to_owned()))
}

pub fn duration_label(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds / 3_600 % 24;
    let minutes = seconds / 60 % 60;
    let secs = seconds % 60;
    let mut parts = Vec::new();
    for (value, name) in [(days, "day"), (hours, "hour"), (minutes, "minute")] {
        if value > 0 {
            parts.push(format!(
                "{value} {name}{}",
                if value == 1 { "" } else { "s" }
            ));
        }
    }
    if secs > 0 && parts.len() < 2 {
        parts.push(format!("{secs} second{}", if secs == 1 { "" } else { "s" }));
    }
    parts.join(" ")
}

fn row(id: String, name: String, subtitle: String, data: serde_json::Value) -> Entry {
    Entry {
        id,
        name,
        subtitle,
        icon: crate::paths::command_icon("timer"),
        provider: "Timer".into(),
        keywords: String::new(),
        data,
    }
}

fn set_row(spec: &str) -> Entry {
    match parse_spec(spec) {
        Some((seconds, label)) => row(
            "Timer:set".into(),
            format!("Start timer · {}", duration_label(seconds)),
            format!("{} · Enter to start", label),
            json!({"action":"set","seconds":seconds,"label":label}),
        ),
        None => row(
            "Timer:invalid".into(),
            "Could not understand that time".into(),
            "Try `10 minutes`, `1 hour 30 minutes`, or `tomorrow at 9am`".into(),
            json!({"disabled":true}),
        ),
    }
}

fn remaining(due_at: i64) -> String {
    let left = due_at.saturating_sub(now().floor() as i64).max(0) as u64;
    if left == 0 {
        return "Finished · alert pending".into();
    }
    format!("{} remaining", duration_label(left))
}

pub fn query(query: &str, store: Option<&Store>, demo: bool) -> Result<Vec<Entry>> {
    let tail = query.strip_prefix("/timer").unwrap_or("").trim();
    if tail == "set" {
        return Ok(vec![set_row("")]);
    }
    if let Some(spec) = tail.strip_prefix("set ") {
        return Ok(vec![set_row(spec)]);
    }
    if !tail.is_empty() {
        return Ok(vec![row(
            "Timer:help".into(),
            "Set or review timers".into(),
            "Use `/timer set 10 minutes`".into(),
            json!({"disabled":true}),
        )]);
    }

    let records = if demo {
        vec![TimerRecord {
            id: "demo".into(),
            label: "Demo timer".into(),
            due_at: now().floor() as i64 + 2 * 60,
            seconds: 2 * 60,
            unit: String::new(),
        }]
    } else {
        store.context("Timer storage is unavailable")?.timers()?
    };
    if records.is_empty() {
        return Ok(vec![row(
            "Timer:empty".into(),
            "No active timers".into(),
            "Type `/timer set 10 minutes` to start one".into(),
            json!({"disabled":true}),
        )]);
    }
    Ok(records
        .into_iter()
        .map(|record| {
            let finished = record.due_at <= now().floor() as i64;
            let end = Local
                .timestamp_opt(record.due_at, 0)
                .single()
                .map(|time| time.format("%H:%M").to_string())
                .unwrap_or_default();
            row(
                format!("Timer:{}", record.id),
                record.label,
                if finished {
                    remaining(record.due_at)
                } else {
                    format!("Ends {end} · {}", remaining(record.due_at))
                },
                json!({"timer_id":record.id,"unit":record.unit,"finished":finished}),
            )
        })
        .collect())
}

fn systemd_schedule(timer: &TimerRecord) -> Result<()> {
    let systemd_run = which("systemd-run").context("systemd-run is required for timers")?;
    let executable = env::current_exe()?;
    let mut args = vec![
        systemd_run.to_string_lossy().into_owned(),
        "--user".into(),
        "--quiet".into(),
        "--collect".into(),
        "--property=Type=exec".into(),
        format!("--unit={}", timer.unit),
        format!("--on-active={}s", timer.seconds),
    ];
    for key in [
        "HOME",
        "PATH",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "WAYLAND_DISPLAY",
        "QT_QPA_PLATFORM",
        "NOTJIK_HOME",
    ] {
        if let Ok(value) = env::var(key) {
            args.push(format!("--setenv={key}={value}"));
        }
    }
    args.extend([
        format!("--working-directory={}", home().display()),
        "--".into(),
        executable.to_string_lossy().into_owned(),
        "timer-fire".into(),
        timer.id.clone(),
    ]);
    run(&args, None, 8).context("Could not schedule timer with the user systemd manager")?;
    Ok(())
}

pub fn start(store: &Store, seconds: u64, label: &str) -> Result<TimerRecord> {
    anyhow::ensure!(
        (1..=MAX_SECONDS).contains(&seconds),
        "Timer duration is out of range"
    );
    let id = Uuid::new_v4().simple().to_string();
    let timer = TimerRecord {
        id: id.clone(),
        label: label.to_owned(),
        due_at: now().floor() as i64 + seconds as i64,
        seconds,
        unit: format!("runtjik-timer-{id}"),
    };
    store.add_timer(&timer)?;
    if let Err(error) = systemd_schedule(&timer) {
        let _ = store.remove_timer(&timer.id);
        return Err(error);
    }
    Ok(timer)
}

fn notify(timer: &TimerRecord) -> Result<()> {
    let title = "Timer finished";
    let body = if timer.label.trim().is_empty() {
        "Your timer is done".to_owned()
    } else {
        timer.label.clone()
    };
    let notjik = which("notjikctl").or_else(|| {
        let path = home().join(".local/bin/notjikctl");
        path.is_file().then_some(path)
    });
    if let Some(notjik) = notjik {
        let args = vec![
            notjik.to_string_lossy().into_owned(),
            "send".into(),
            title.into(),
            body.clone(),
            "--app".into(),
            "Runtjik".into(),
            "--type".into(),
            "status".into(),
            "--data".into(),
            r#"{"state":"Finished","tone":"success"}"#.into(),
            "--icon".into(),
            "alarm-symbolic".into(),
            "--timeout".into(),
            "8s".into(),
        ];
        if run(&args, None, 8).is_ok() {
            return Ok(());
        }
    }
    let notify_send = which("notify-send").context("Notjik and notify-send are unavailable")?;
    let args = vec![
        notify_send.to_string_lossy().into_owned(),
        "--app-name=Runtjik".into(),
        "--icon=alarm-symbolic".into(),
        title.into(),
        body,
    ];
    run(&args, None, 8).context("Could not send the timer notification")?;
    Ok(())
}

pub fn fire(store: &Store, id: &str) -> Result<()> {
    let Some(timer) = store.timer(id)? else {
        return Ok(());
    };
    notify(&timer)?;
    store.remove_timer(id)?;
    Ok(())
}

pub fn cancel(store: &Store, id: &str, demo: bool) -> Result<bool> {
    if demo {
        return Ok(true);
    }
    let Some(timer) = store.timer(id)? else {
        return Ok(false);
    };
    store.remove_timer(id)?;
    if let Some(systemctl) = which("systemctl") {
        let _ = run(
            &[
                systemctl.to_string_lossy().into_owned(),
                "--user".into(),
                "stop".into(),
                format!("{}.timer", timer.unit),
                format!("{}.service", timer.unit),
            ],
            None,
            8,
        );
    }
    Ok(true)
}

pub fn retry(store: &Store, id: &str, demo: bool) -> Result<()> {
    if demo {
        return Ok(());
    }
    fire(store, id)
}

pub fn parse_set_row(entry: &Entry) -> Option<(u64, String)> {
    if s(&entry.data["action"]) != "set" {
        return None;
    }
    Some((
        entry.data["seconds"].as_u64()?,
        s(&entry.data["label"]).to_owned(),
    ))
}
