//! Persistent night-light settings, local solar calculations and Hyprsunset supervision.
use crate::util::*;
use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration as StdDuration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn settings_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("bartjik/nightlight.json")
}
fn enabled_path() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("nightlight/state")
}
fn runtime(name: &str) -> PathBuf {
    xdg("XDG_RUNTIME_DIR", ".cache").join(format!("bartjik-nightlight-{name}.json"))
}
fn timezone() -> chrono_tz::Tz {
    std::env::var("TZ")
        .ok()
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            std::fs::read_link("/etc/localtime")
                .ok()
                .and_then(|p| p.to_str()?.split("zoneinfo/").nth(1)?.parse().ok())
        })
        .unwrap_or(chrono_tz::UTC)
}
pub fn defaults() -> Value {
    let mut out = json!({"temperature":5500,"gamma":80,"start":"21:00","end":"07:30","mode":"fixed","location":null});
    if let Ok(config) =
        std::fs::read_to_string(xdg("XDG_CONFIG_HOME", ".config").join("hypr/hyprsunset.conf"))
    {
        let blocks = regex::Regex::new(r"(?s)profile\s*\{([^}]*)\}").unwrap();
        for block in blocks.captures_iter(&config) {
            let fields: std::collections::HashMap<_, _> = block[1]
                .lines()
                .filter_map(|l| {
                    l.split('#')
                        .next()?
                        .split_once('=')
                        .map(|(k, v)| (k.trim(), v.trim()))
                })
                .collect();
            if let Some(time) = fields.get("time").and_then(|v| parse_time(v).ok()) {
                let identity = fields
                    .get("identity")
                    .is_some_and(|v| *v == "true" || *v == "1");
                out[if identity { "end" } else { "start" }] =
                    json!(time.format("%H:%M").to_string());
                if !identity {
                    if let Some(n) = fields
                        .get("temperature")
                        .and_then(|v| v.parse::<u64>().ok())
                        .filter(|n| (1000..=6500).contains(n))
                    {
                        out["temperature"] = json!(n);
                    }
                    if let Some(n) = fields
                        .get("gamma")
                        .and_then(|v| v.parse::<f64>().ok())
                        .filter(|n| (0.1..=1.5).contains(n))
                    {
                        out["gamma"] = json!((n * 100.).round() as u64);
                    }
                }
            }
        }
    }
    out
}
fn parse_time(value: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M").context("Use a time in HH:MM format")
}
fn validate_location(v: &Value) -> Result<()> {
    ensure!(
        v["latitude"]
            .as_f64()
            .is_some_and(|n| n.is_finite() && (-90. ..=90.).contains(&n))
            && v["longitude"]
                .as_f64()
                .is_some_and(|n| n.is_finite() && (-180. ..=180.).contains(&n)),
        "Invalid location coordinates"
    );
    ensure!(
        !s(&v["name"]).is_empty() && s(&v["name"]).chars().count() <= 160,
        "Invalid location name"
    );
    ensure!(
        ["device", "city"].contains(&s(&v["source"])),
        "Invalid location source"
    );
    Ok(())
}
pub fn validate(v: &Value) -> Result<()> {
    ensure!(
        v["temperature"]
            .as_u64()
            .is_some_and(|n| (1000..=6500).contains(&n)),
        "Temperature must be between 1000 and 6500 K"
    );
    ensure!(
        v["gamma"].as_u64().is_some_and(|n| (10..=150).contains(&n)),
        "Brightness must be between 10 and 150%"
    );
    ensure!(
        parse_time(s(&v["start"]))? != parse_time(s(&v["end"]))?,
        "Start and end times must be different"
    );
    ensure!(
        ["fixed", "solar"].contains(&s(&v["mode"])),
        "Invalid schedule mode"
    );
    if !v["location"].is_null() {
        validate_location(&v["location"])?;
    }
    ensure!(
        v["mode"] != "solar" || !v["location"].is_null(),
        "Choose a location before following sunset"
    );
    Ok(())
}
pub fn settings() -> Result<Value> {
    let path = settings_path();
    let value = if path.exists() {
        crate::config::merge(&defaults(), &read(path)?)
    } else {
        defaults()
    };
    validate(&value)?;
    Ok(value)
}

// NOAA fractional-year equation of time and solar declination; apparent horizon
// includes atmospheric refraction and the solar radius (90.833 degrees).
// https://gml.noaa.gov/grad/solcalc/solareqns.PDF
pub fn solar(date: NaiveDate, latitude: f64, longitude: f64) -> (Option<i64>, Option<i64>, bool) {
    let year_days = if date.with_ordinal(366).is_some() {
        366.
    } else {
        365.
    };
    let g = 2. * std::f64::consts::PI / year_days * (date.ordinal() as f64 - 1.);
    let eq = 229.18
        * (0.000075 + 0.001868 * g.cos()
            - 0.032077 * g.sin()
            - 0.014615 * (2. * g).cos()
            - 0.040849 * (2. * g).sin());
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2. * g).cos()
        + 0.000907 * (2. * g).sin()
        - 0.002697 * (3. * g).cos()
        + 0.00148 * (3. * g).sin();
    let lat = latitude.to_radians();
    let cosine =
        (90.833_f64.to_radians().cos() - lat.sin() * decl.sin()) / (lat.cos() * decl.cos());
    if cosine > 1. {
        return (None, None, true);
    }
    if cosine < -1. {
        return (None, None, false);
    }
    let half = cosine.clamp(-1., 1.).acos().to_degrees() * 4.;
    let noon = 720. - 4. * longitude - eq;
    let base = date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
    (
        Some(base + ((noon - half) * 60.).round() as i64),
        Some(base + ((noon + half) * 60.).round() as i64),
        false,
    )
}
pub fn schedule(v: &Value, now: DateTime<Utc>, zone: chrono_tz::Tz) -> Value {
    let zone = if v["mode"] == "solar" {
        s(&v["location"]["timezone"])
            .parse::<chrono_tz::Tz>()
            .unwrap_or(zone)
    } else {
        zone
    };
    let local = now.with_timezone(&zone);
    let date = local.date_naive();
    let mut events: Vec<(i64, bool)> = vec![];
    let mut sunrise = None;
    let mut sunset = None;
    let mut polar_night = false;
    for days in -2..=2 {
        let day = date + Duration::days(days);
        if v["mode"] == "solar" {
            let longitude = num(&v["location"]["longitude"]);
            // Political time zones can cross the date line (for example Kiritimati).
            // Use the solar date containing local noon, not the calendar date in UTC.
            let solar_day = zone
                .from_local_datetime(&day.and_hms_opt(12, 0, 0).unwrap())
                .earliest()
                .map(|d| {
                    (d.with_timezone(&Utc) + Duration::seconds((longitude * 240.).round() as i64))
                        .date_naive()
                })
                .unwrap_or(day);
            let (rise, set, night) = solar(solar_day, num(&v["location"]["latitude"]), longitude);
            if days == 0 {
                sunrise = rise;
                sunset = set;
                polar_night = night;
            }
            if let Some(rise) = rise {
                events.push((rise, false));
            }
            if let Some(set) = set {
                events.push((set, true));
            }
        } else {
            for (key, on) in [("start", true), ("end", false)] {
                if let Ok(time) = parse_time(s(&v[key])) {
                    // A skipped DST time takes effect at the first valid minute.
                    for minutes in 0..=120 {
                        if let Some(dt) = zone
                            .from_local_datetime(&(day.and_time(time) + Duration::minutes(minutes)))
                            .earliest()
                        {
                            events.push((dt.timestamp(), on));
                            break;
                        }
                    }
                }
            }
        }
    }
    events.sort_by_key(|e| e.0);
    let active = events
        .iter()
        .rev()
        .find(|e| e.0 <= now.timestamp())
        .map(|e| e.1)
        .unwrap_or(polar_night);
    let next = events.iter().find(|e| e.0 > now.timestamp());
    let time_label = |stamp: Option<i64>| {
        stamp
            .and_then(|n| DateTime::from_timestamp(n, 0))
            .map(|d| d.with_timezone(&zone).format("%H:%M").to_string())
            .unwrap_or("—".into())
    };
    json!({"active":active,"nextAt":next.map(|e|e.0),"nextOn":next.map(|e|e.1),"nextTime":time_label(next.map(|e|e.0)),"sunrise":time_label(sunrise),"sunset":time_label(sunset),"timezone":zone.name(),"polar":v["mode"]=="solar" && sunrise.is_none(),"polarNight":polar_night})
}
fn socket_path() -> Result<PathBuf> {
    Ok(PathBuf::from(std::env::var("XDG_RUNTIME_DIR")?)
        .join("hypr")
        .join(std::env::var("HYPRLAND_INSTANCE_SIGNATURE")?)
        .join(".hyprsunset.sock"))
}
async fn ipc(request: &str) -> Result<String> {
    tokio::time::timeout(StdDuration::from_secs(1), async {
        let mut stream = tokio::net::UnixStream::connect(socket_path()?).await?;
        stream.write_all(request.as_bytes()).await?;
        stream.shutdown().await?;
        let mut bytes = vec![];
        stream.take(4096).read_to_end(&mut bytes).await?;
        Ok(String::from_utf8(bytes)?.trim().to_owned())
    })
    .await?
}
async fn apply(v: &Value, active: bool) -> Result<()> {
    let temperature = if active {
        format!("temperature {}", v["temperature"])
    } else {
        "identity".into()
    };
    ensure!(
        ipc(&temperature).await? == "ok",
        "Hyprsunset rejected the temperature"
    );
    ensure!(
        ipc(&format!(
            "gamma {}",
            if active { num(&v["gamma"]) } else { 100. }
        ))
        .await?
            == "ok",
        "Hyprsunset rejected the brightness"
    );
    Ok(())
}
fn enabled(running: bool) -> bool {
    match std::fs::read_to_string(enabled_path())
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("on") => true,
        Some("off") => false,
        _ => running,
    }
}
pub async fn set_enabled(on: bool) -> Result<()> {
    let helper = home().join(".local/bin/nightlight");
    if helper.is_file() {
        command(
            &[
                helper.to_string_lossy().into(),
                if on { "on".into() } else { "off".into() },
            ],
            10.,
            4096,
            &[],
            None,
        )
        .await?;
    } else {
        let path = enabled_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        writeln!(f, "{}", if on { "on" } else { "off" })?;
        f.persist(path)?;
        cmd(&[
            "systemctl",
            "--user",
            if on { "start" } else { "stop" },
            "hyprsunset.service",
        ])
        .await?;
    }
    Ok(())
}
pub async fn toggle() -> Result<()> {
    let _lock = crate::actions::FileLock::acquire("bartjik-nightlight.lock").await?;
    set_enabled(!enabled(ipc("identity get").await.is_ok())).await
}
pub fn check_enabled() -> Result<()> {
    ensure!(enabled(true), "Night light is disabled");
    Ok(())
}
fn panel(v: &Value, plan: &Value, enabled: bool, active: bool, running: bool, stamp: f64) -> Value {
    let next = if enabled {
        if !running {
            "Night-light service unavailable".into()
        } else if active {
            "Active now".into()
        } else if truth(&plan["active"]) {
            "Warm colours scheduled now".into()
        } else if truth(&plan["polar"]) {
            "Midnight sun · daylight colours all day".into()
        } else {
            format!("Starts at {}", s(&plan["nextTime"]))
        }
    } else {
        "Off · your schedule is saved".into()
    };
    json!({"title":"Night light","subtitle":next,"rows":[],"settings":v,"schedule":plan,"enabled":enabled,"active":active,"running":running,"cities":[],"locationAgeHours":if v["location"]["updatedAt"].is_number(){json!(((stamp-num(&v["location"]["updatedAt"]))/3600.).max(0.))}else{Value::Null},"error":if enabled && !running{"Night-light service is not running"}else{""}})
}
pub fn demo() -> Value {
    let v = json!({"temperature":4500,"gamma":85,"start":"21:00","end":"07:30","mode":"solar","location":{"name":"Hong Kong","latitude":22.3,"longitude":114.2,"timezone":"Asia/Hong_Kong","source":"city","updatedAt":1793764800}});
    let plan = schedule(
        &v,
        DateTime::from_timestamp(1793764800, 0).unwrap(),
        chrono_tz::Asia::Hong_Kong,
    );
    let mut row = item(
        "filter",
        None,
        Some("light_mode"),
        "symbol",
        json!({}),
        "Night light · scheduled",
        json!({"leftClick":"nightlight.toggle","rightClick":"nightlight.panel","hoverEnter":"nightlight.panel"}),
    );
    row["panel"] = panel(&v, &plan, true, false, true, 1793764800.);
    row["panel"]["cities"] = json!([]);
    row
}
pub async fn snapshot() -> Result<Value> {
    let mut v = settings()?;
    label_device_location(&mut v["location"]);
    let plan = schedule(&v, Utc::now(), timezone());
    let identity = ipc("identity get")
        .await
        .ok()
        .filter(|v| v == "true" || v == "false");
    let running = identity.is_some();
    let active = identity.as_deref() == Some("false");
    let enabled = enabled(running);
    let tooltip = if active {
        format!("Night light : ON · {} K", v["temperature"])
    } else if enabled && !running {
        "Night light · service unavailable".into()
    } else if enabled && truth(&plan["active"]) {
        "Night light · warm colours scheduled now".into()
    } else if enabled && truth(&plan["polar"]) {
        "Night light · midnight sun".into()
    } else if enabled {
        format!("Night light · starts {}", s(&plan["nextTime"]))
    } else {
        "Night light : OFF".into()
    };
    let mut row = item(
        "filter",
        None,
        Some(if active { "nightlight" } else { "light_mode" }),
        "symbol",
        json!({"active":active}),
        &tooltip,
        json!({"leftClick":"nightlight.toggle","rightClick":"nightlight.panel","hoverEnter":"nightlight.panel"}),
    );
    row["panel"] = panel(&v, &plan, enabled, active, running, now());
    row["panel"]["cities"] = read(runtime("cities")).unwrap_or(json!([]));
    Ok(row)
}

fn coordinate_name(latitude: f64, longitude: f64) -> String {
    format!(
        "{:.2}° {}, {:.2}° {}",
        latitude.abs(),
        if latitude < 0. { "S" } else { "N" },
        longitude.abs(),
        if longitude < 0. { "W" } else { "E" }
    )
}
fn label_device_location(location: &mut Value) {
    if location["source"] != "device" || validate_location(location).is_err() {
        return;
    }
    let latitude = num(&location["latitude"]);
    let longitude = num(&location["longitude"]);
    location["name"] = json!(crate::cities::near(latitude, longitude)
        .unwrap_or_else(|| coordinate_name(latitude, longitude)));
}

fn device_location() -> Result<Value> {
    use gio::glib::{variant::ObjectPath, variant::ToVariant, Variant};
    let connection = gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE)?;
    let call =
        |path: &str, interface: &str, method: &str, args: Option<&Variant>| -> Result<Variant> {
            Ok(connection.call_sync(
                Some("org.freedesktop.GeoClue2"),
                path,
                interface,
                method,
                args,
                None,
                gio::DBusCallFlags::NONE,
                4000,
                gio::Cancellable::NONE,
            )?)
        };
    let manager = "/org/freedesktop/GeoClue2/Manager";
    let client = call(
        manager,
        "org.freedesktop.GeoClue2.Manager",
        "CreateClient",
        None,
    )?
    .child_get::<ObjectPath>(0);
    let result = (|| -> Result<Value> {
        for (key, value) in [
            ("DesktopId", "bartjik".to_variant()),
            ("RequestedAccuracyLevel", 4u32.to_variant()),
        ] {
            call(
                client.as_str(),
                "org.freedesktop.DBus.Properties",
                "Set",
                Some(&("org.freedesktop.GeoClue2.Client", key, value).to_variant()),
            )?;
        }
        call(
            client.as_str(),
            "org.freedesktop.GeoClue2.Client",
            "Start",
            None,
        )?;
        let deadline = std::time::Instant::now() + StdDuration::from_secs(10);
        while std::time::Instant::now() < deadline {
            let value = call(
                client.as_str(),
                "org.freedesktop.DBus.Properties",
                "Get",
                Some(&("org.freedesktop.GeoClue2.Client", "Location").to_variant()),
            )?;
            if let Some(location) = value
                .child_get::<Variant>(0)
                .get::<ObjectPath>()
                .filter(|v| v.as_str() != "/")
            {
                let get = |key: &str| -> Result<f64> {
                    let v = call(
                        location.as_str(),
                        "org.freedesktop.DBus.Properties",
                        "Get",
                        Some(&("org.freedesktop.GeoClue2.Location", key).to_variant()),
                    )?;
                    v.child_get::<Variant>(0)
                        .get::<f64>()
                        .context("Invalid device location")
                };
                return Ok(
                    json!({"name":"Device location","latitude":get("Latitude")?,"longitude":get("Longitude")?,"timezone":timezone().name(),"source":"device","updatedAt":now()}),
                );
            }
            std::thread::sleep(StdDuration::from_millis(250));
        }
        anyhow::bail!("Device location is unavailable")
    })();
    let _ = call(
        client.as_str(),
        "org.freedesktop.GeoClue2.Client",
        "Stop",
        None,
    );
    let _ = call(
        manager,
        "org.freedesktop.GeoClue2.Manager",
        "DeleteClient",
        Some(&(client,).to_variant()),
    );
    result
}
async fn locate() -> Result<Value> {
    let mut location = tokio::task::spawn_blocking(device_location)
        .await?
        .map_err(|_| {
            anyhow::anyhow!(
                "Device location unavailable. Allow location access or search for your city."
            )
        })?;
    label_device_location(&mut location);
    Ok(location)
}
pub async fn action(name: &str, selection: &str) -> Result<String> {
    let _lock = crate::actions::FileLock::acquire("bartjik-nightlight.lock").await?;
    let mut v = settings()?;
    match name {
        "enabled" => {
            ensure!(
                ["on", "off"].contains(&selection),
                "Invalid night-light setting"
            );
            set_enabled(selection == "on").await?;
            return Ok(if selection == "on" {
                "Schedule enabled"
            } else {
                "Night light off"
            }
            .into());
        }
        "temperature" | "gamma" => {
            v[name] = json!(selection.parse::<u64>().context("Invalid setting")?)
        }
        "hours" => {
            let hours: Value = serde_json::from_str(selection)?;
            v["start"] = json!(parse_time(s(&hours["start"]))?.format("%H:%M").to_string());
            v["end"] = json!(parse_time(s(&hours["end"]))?.format("%H:%M").to_string());
        }
        "start" | "end" => {
            v[name] = json!(parse_time(selection)?.format("%H:%M").to_string());
        }
        "mode" => {
            ensure!(
                ["fixed", "solar"].contains(&selection),
                "Invalid schedule mode"
            );
            if selection == "solar" && v["location"].is_null() {
                v["location"] = locate().await?;
            }
            v["mode"] = json!(selection);
        }
        "locate" => v["location"] = locate().await?,
        "city-search" => {
            ensure!(
                (2..=80).contains(&selection.chars().count()),
                "Enter at least two characters"
            );
            let mut url = url::Url::parse("https://geocoding-api.open-meteo.com/v1/search")?;
            url.query_pairs_mut()
                .append_pair("name", selection)
                .append_pair("count", "5")
                .append_pair("language", "en")
                .append_pair("format", "json");
            let response = hj(url.as_str(), &[]).await.context(
                "City search unavailable. Check your internet connection and try again.",
            )?;
            let cities:Vec<_>=arr(&response["results"]).iter().filter(|v|v["id"].is_u64() && v["latitude"].is_number() && v["longitude"].is_number()).map(|r|json!({"id":r["id"],"name":format!("{}, {}",s(&r["name"]),s(&r["country"])),"detail":r["admin1"],"latitude":r["latitude"],"longitude":r["longitude"],"timezone":r["timezone"],"source":"city","updatedAt":now()})).collect();
            save(&runtime("cities"), &json!(cities))?;
            return Ok(if cities.is_empty() {
                "No matching cities"
            } else {
                "Choose your city below"
            }
            .into());
        }
        "city-select" => {
            let results = read(runtime("cities"))?;
            v["location"] = arr(&results)
                .iter()
                .find(|v| st(&v["id"]) == selection)
                .context("Search again to choose a city")?
                .clone();
            save(&runtime("cities"), &json!([]))?;
        }
        _ => anyhow::bail!("Unknown night-light control"),
    }
    validate(&v)?;
    save(&settings_path(), &v)?;
    Ok("Saved".into())
}
pub async fn run() -> Result<()> {
    check_enabled()?;
    let _guard = crate::actions::FileLock::acquire("bartjik-nightlight-daemon.lock").await?;
    let config = xdg("XDG_RUNTIME_DIR", ".cache").join("bartjik-nightlight-config");
    std::fs::create_dir_all(config.join("hypr"))?;
    std::fs::write(
        config.join("hypr/hyprsunset.conf"),
        "# Schedule is applied by Bartjik.\nmax-gamma = 150\n",
    )?;
    let mut child = tokio::process::Command::new("hyprsunset")
        .args(["--identity", "--gamma", "100", "--gamma_max", "150"])
        .env("XDG_CONFIG_HOME", config)
        .kill_on_drop(true)
        .spawn()?;
    let mut applied = String::new();
    let mut location_attempt = 0.;
    loop {
        if let Some(status) = child.try_wait()? {
            anyhow::bail!("Hyprsunset exited: {status}");
        }
        if let Ok(mut v) = settings() {
            if v["mode"] == "solar"
                && v["location"]["source"] == "device"
                && now() - num(&v["location"]["updatedAt"]) > 3600.
                && now() - location_attempt > 3600.
            {
                location_attempt = now();
                // Do not hold up screen transitions while the system locates us.
                tokio::spawn(async {
                    if let Ok(location) = locate().await {
                        if let Ok(_lock) =
                            crate::actions::FileLock::acquire("bartjik-nightlight.lock").await
                        {
                            if let Ok(mut current) = settings() {
                                if current["location"]["source"] == "device" {
                                    current["location"] = location;
                                    let _ = save(&settings_path(), &current);
                                }
                            }
                        }
                    }
                });
            }
            let plan = schedule(&v, Utc::now(), timezone());
            v["active"] = plan["active"].clone();
            let key = json!([v["temperature"], v["gamma"], v["active"]]).to_string();
            if applied != key && apply(&v, truth(&v["active"])).await.is_ok() {
                applied = key;
            } else if applied == key && truth(&v["active"]) && num(&v["gamma"]) != 100. {
                // Hypridle restores gamma to 100 on resume. Keep its dimmed value
                // untouched, then restore the configured night brightness on wake.
                if ipc("gamma")
                    .await
                    .ok()
                    .and_then(|g| g.parse::<f64>().ok())
                    .is_some_and(|g| (g - 100.).abs() < 0.01)
                {
                    let _ = ipc(&format!("gamma {}", v["gamma"])).await;
                }
            }
        }
        tokio::time::sleep(StdDuration::from_secs(2)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed() -> Value {
        json!({"temperature":5500,"gamma":80,"start":"21:00","end":"07:30","mode":"fixed","location":null})
    }
    fn at(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }
    fn solar_settings(latitude: f64, longitude: f64, zone: &str) -> Value {
        let mut v = fixed();
        v["mode"] = json!("solar");
        v["location"] = json!({"latitude":latitude,"longitude":longitude,"timezone":zone,"name":"Fixture city","source":"city"});
        v
    }
    fn near(actual: &Value, expected: &str, minutes: i64) {
        let a = parse_time(s(actual)).unwrap();
        let e = parse_time(expected).unwrap();
        assert!(
            (a - e).num_minutes().abs() <= minutes,
            "{actual}, expected near {expected}"
        );
    }

    #[test]
    fn overnight_schedule_and_exact_boundaries() {
        let v = fixed();
        for (time, active, next) in [
            ("2026-11-04T07:29:59+08:00", true, "07:30"),
            ("2026-11-04T07:30:00+08:00", false, "21:00"),
            ("2026-11-04T20:59:59+08:00", false, "21:00"),
            ("2026-11-04T21:00:00+08:00", true, "07:30"),
            ("2026-11-05T00:00:00+08:00", true, "07:30"),
        ] {
            let p = schedule(&v, at(time), chrono_tz::Asia::Hong_Kong);
            assert_eq!(p["active"], active, "{time}");
            assert_eq!(p["nextTime"], next);
        }
        // Fixed schedules follow the computer's time zone even after choosing a city.
        let mut v = solar_settings(40.7, -74., "America/New_York");
        v["mode"] = json!("fixed");
        assert_eq!(
            schedule(
                &v,
                at("2026-11-04T22:00:00+08:00"),
                chrono_tz::Asia::Hong_Kong
            )["active"],
            true
        );
        v["start"] = json!("08:00");
        v["end"] = json!("17:00");
        assert_eq!(
            schedule(&v, at("2026-11-04T12:00:00Z"), chrono_tz::UTC)["active"],
            true
        );
        assert_eq!(
            schedule(&v, at("2026-11-04T18:00:00Z"), chrono_tz::UTC)["active"],
            false
        );
    }

    #[test]
    fn dst_gap_and_repeated_hour_do_not_toggle_twice() {
        let mut v = fixed();
        v["start"] = json!("02:30");
        let zone = chrono_tz::America::New_York;
        assert_eq!(
            schedule(&v, at("2026-03-08T06:59:59Z"), zone)["active"],
            false
        );
        assert_eq!(
            schedule(&v, at("2026-03-08T07:00:00Z"), zone)["active"],
            true
        );
        v["start"] = json!("01:30");
        assert_eq!(
            schedule(&v, at("2026-11-01T05:29:59Z"), zone)["active"],
            false
        );
        for time in [
            "2026-11-01T05:30:00Z",
            "2026-11-01T06:00:00Z",
            "2026-11-01T06:30:00Z",
        ] {
            let p = schedule(&v, at(time), zone);
            assert_eq!(p["active"], true);
            assert_eq!(p["nextTime"], "07:30");
        }
    }

    #[test]
    fn solar_reference_times_and_boundaries() {
        // Approximate almanac times; NOAA's compact model is accurate to a few minutes.
        for (date, sunrise, sunset) in [
            ("2026-06-21T12:00:00+08:00", "05:39", "19:11"),
            ("2026-12-21T12:00:00+08:00", "06:58", "17:44"),
            ("2024-02-29T12:00:00+08:00", "06:45", "18:27"),
        ] {
            let v = solar_settings(22.3, 114.2, "Asia/Hong_Kong");
            let p = schedule(&v, at(date), chrono_tz::UTC);
            near(&p["sunrise"], sunrise, 5);
            near(&p["sunset"], sunset, 5);
            assert_eq!(p["active"], false);
            let next = p["nextAt"].as_i64().unwrap();
            assert_eq!(
                schedule(
                    &v,
                    DateTime::from_timestamp(next - 1, 0).unwrap(),
                    chrono_tz::UTC
                )["active"],
                false
            );
            assert_eq!(
                schedule(
                    &v,
                    DateTime::from_timestamp(next, 0).unwrap(),
                    chrono_tz::UTC
                )["active"],
                true
            );
        }
        let p = schedule(
            &solar_settings(0., 0., "UTC"),
            at("2026-03-20T12:00:00Z"),
            chrono_tz::UTC,
        );
        near(&p["sunrise"], "06:04", 4);
        near(&p["sunset"], "18:11", 4);
    }

    #[test]
    fn date_line_and_polar_seasons() {
        let zone = chrono_tz::Pacific::Kiritimati;
        let v = solar_settings(1.87, -157.43, "Pacific/Kiritimati");
        let time = at("2026-06-21T12:00:00+14:00");
        let p = schedule(&v, time, chrono_tz::UTC);
        assert_eq!(p["active"], false);
        near(&p["sunset"], "18:38", 5);
        let sunset = DateTime::from_timestamp(p["nextAt"].as_i64().unwrap(), 0).unwrap();
        assert_eq!(
            sunset.with_timezone(&zone).date_naive(),
            time.with_timezone(&zone).date_naive()
        );
        for (lat, date, night) in [
            (78., "2026-06-21T12:00:00Z", false),
            (78., "2026-12-21T12:00:00Z", true),
            (-78., "2026-06-21T12:00:00Z", true),
            (-78., "2026-12-21T12:00:00Z", false),
        ] {
            let p = schedule(&solar_settings(lat, 0., "UTC"), at(date), chrono_tz::UTC);
            assert_eq!(p["polar"], true);
            assert_eq!(p["active"], night);
            assert_eq!(p["nextAt"], Value::Null);
        }
    }

    #[test]
    fn invalid_settings_rejected_and_demo_is_deterministic() {
        for (key, value) in [
            ("temperature", json!(999)),
            ("temperature", json!(6501)),
            ("gamma", json!(0)),
            ("gamma", json!(151)),
            ("start", json!("24:00")),
            ("end", json!("21:00")),
            ("mode", json!("solar")),
            ("mode", json!("other")),
        ] {
            let mut v = fixed();
            v[key] = value;
            assert!(validate(&v).is_err(), "{v}");
        }
        assert!(validate(&solar_settings(91., 0., "UTC")).is_err());
        assert!(validate(&solar_settings(0., -181., "UTC")).is_err());
        let a = demo();
        assert_eq!(a, demo());
        assert_eq!(a["panel"]["locationAgeHours"], 0.);
        assert_eq!(a["actions"]["hoverEnter"], "nightlight.panel");
    }

    #[test]
    fn device_city_label_preserves_the_fix_and_uses_coordinates_in_remote_areas() {
        let mut location = json!({"source":"device","name":"Device location","latitude":51.50721,"longitude":-0.12758,"timezone":"Europe/London","updatedAt":1793764800});
        let original = location.clone();
        label_device_location(&mut location);
        assert_eq!(location["name"], "Near London, GB");
        for key in ["source", "latitude", "longitude", "timezone", "updatedAt"] {
            assert_eq!(location[key], original[key]);
        }
        location["latitude"] = json!(0.);
        location["longitude"] = json!(0.);
        label_device_location(&mut location);
        assert_eq!(location["name"], "0.00° N, 0.00° E");
        assert_eq!(coordinate_name(-33.8688, 151.2093), "33.87° S, 151.21° E");
    }
}
