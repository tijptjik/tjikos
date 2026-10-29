//! Release availability is checked only 25 weeks after the installed release.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const WAIT: u64 = 25 * 7 * 86400;
const RETRY: u64 = 86400;
#[derive(Serialize, Deserialize)]
struct ReleaseDate {
    version: u32,
    checked: u64,
    released_on: Option<String>,
}
fn date_seconds(date: &str) -> Result<u64> {
    let bytes = date.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        bail!("invalid Fedora release date");
    }
    let output = Command::new("date")
        .args(["-u", "-d", date, "+%s"])
        .output()?;
    if !output.status.success() {
        bail!("invalid Fedora release date");
    }
    Ok(String::from_utf8(output.stdout)?.trim().parse()?)
}
fn fetch(url: &str) -> Result<Value> {
    let output = Command::new("curl")
        .args(["-fsSL", "--max-time", "15", url])
        .output()?;
    if !output.status.success() {
        bail!("Fedora metadata unavailable");
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}
fn release_date(value: &Value, version: u32) -> Result<Option<String>> {
    let releases = value["releases"]
        .as_array()
        .context("invalid Fedora metadata")?;
    let version = version.to_string();
    let release = releases
        .iter()
        .find(|r| r["version"].as_str() == Some(&version))
        .context("installed Fedora release absent from metadata")?;
    match &release["released_on"] {
        Value::Null => Ok(None),
        Value::String(date) => {
            date_seconds(date)?;
            Ok(Some(date.clone()))
        }
        _ => bail!("invalid Fedora release date"),
    }
}
fn eligible(released: u64, now: u64) -> bool {
    now >= released.saturating_add(WAIT)
}
fn cached_valid(record: &ReleaseDate, version: u32, now: u64) -> bool {
    record.version == version
        && record.checked <= now
        && match &record.released_on {
            Some(date) => date_seconds(date).is_ok(),
            None => now - record.checked < RETRY,
        }
}
fn latest(value: &Value) -> Result<u32> {
    value
        .as_array()
        .or_else(|| value["releases"].as_array())
        .context("invalid Fedora releases")?
        .iter()
        .filter_map(|r| r["version"].as_str()?.parse::<u32>().ok())
        .max()
        .context("no stable Fedora release")
}
pub fn run() -> Result<i32> {
    let output = Command::new("rpm").args(["-E", "%fedora"]).output()?;
    if !output.status.success() {
        bail!("Fedora version unavailable");
    }
    let version: u32 = String::from_utf8(output.stdout)?.trim().parse()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let directory = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache")))
        .context("cache directory unavailable")?
        .join("tjikup");
    let path = directory.join(format!("fedora-release-{version}.json"));
    let cached = fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<ReleaseDate>(&b).ok())
        .filter(|r| cached_valid(r, version, now));
    let record = match cached {
        Some(record) => record,
        None => {
            let value = fetch(&format!(
                "https://bodhi.fedoraproject.org/releases/?name=F{version}"
            ))?;
            let record = ReleaseDate {
                version,
                checked: now,
                released_on: release_date(&value, version)?,
            };
            fs::create_dir_all(&directory)?;
            let mut file = tempfile::NamedTempFile::new_in(&directory)?;
            serde_json::to_writer(file.as_file_mut(), &record)?;
            file.persist(&path)?;
            record
        }
    };
    let Some(date) = record.released_on else {
        println!("release date pending");
        return Ok(2);
    };
    let released = date_seconds(&date)?;
    if !eligible(released, now) {
        let output = Command::new("date")
            .args(["-u", "-d", &format!("@{}", released + WAIT), "+%F"])
            .output()?;
        if !output.status.success() {
            bail!("release gate date unavailable");
        }
        println!("checks from {}", String::from_utf8(output.stdout)?.trim());
        return Ok(2);
    }
    println!(
        "{}",
        latest(&fetch("https://fedoraproject.org/releases.json")?)?
    );
    Ok(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn waits_exactly_twenty_five_weeks() {
        let released = date_seconds("2026-04-28").unwrap();
        assert!(!eligible(released, released + WAIT - 1));
        assert!(eligible(released, released + WAIT));
        assert!(eligible(released, released + WAIT + 1));
        assert!(!eligible(released, released - 1));
    }
    #[test]
    fn validates_dates_and_release_identity() {
        assert!(date_seconds("2026-02-30").is_err());
        assert!(date_seconds("yesterday").is_err());
        assert!(release_date(&json!({"releases":[]}), 45).is_err());
        assert_eq!(
            release_date(
                &json!({"releases":[{"version":"45","released_on":null}]}),
                45
            )
            .unwrap(),
            None
        );
    }
    #[test]
    fn caches_dates_but_retries_unpublished_dates() {
        let mut r = ReleaseDate {
            version: 45,
            checked: 100,
            released_on: None,
        };
        assert!(cached_valid(&r, 45, 101));
        assert!(!cached_valid(&r, 45, 100 + RETRY));
        assert!(!cached_valid(&r, 44, 101));
        assert!(!cached_valid(&r, 45, 99));
        r.released_on = Some("2026-04-28".into());
        assert!(cached_valid(&r, 45, 100 + RETRY * 100));
        r.released_on = Some("invalid".into());
        assert!(!cached_valid(&r, 45, 101));
    }
    #[test]
    fn ignores_prereleases() {
        assert_eq!(
            latest(&json!([{"version":"45 Beta"},{"version":"44"}])).unwrap(),
            44
        );
    }
}
