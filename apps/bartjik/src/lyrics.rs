use crate::{spotify, util::*};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
use unicode_normalization::UnicodeNormalization;
pub fn normalize(v: &str) -> String {
    v.nfkd()
        .flat_map(char::to_lowercase)
        .flat_map(|c| {
            if c == 'ß' {
                vec!['s', 's']
            } else if c == 'ς' {
                vec!['σ']
            } else {
                vec![c]
            }
        })
        .filter(|c| c.is_alphanumeric())
        .collect()
}
pub fn clean_title(title: &str, timing: bool) -> String {
    let marker = regex::Regex::new(if timing {
        r"(?i)\b(?:remaster(?:ed)?|mono|stereo|feat(?:uring)?|ft)\b"
    } else {
        r"(?i)\b(?:remaster(?:ed)?|remix|mix|edit|version|live|mono|stereo|feat(?:uring)?|ft)\b"
    })
    .unwrap();
    let timing_version = regex::Regex::new(r"(?i)\b(?:live|remix|mix|edit|version)\b").unwrap();
    let removable = |v: &str| marker.is_match(v) && !(timing && timing_version.is_match(v));
    let brackets = regex::Regex::new(r"\([^()]*\)|\[[^\[\]]*\]").unwrap();
    let title = brackets.replace_all(title, |c: &regex::Captures| {
        if removable(&c[0]) {
            String::new()
        } else {
            c[0].into()
        }
    });
    let split = regex::Regex::new(r"\s+[-–—]\s+").unwrap();
    let mut parts: Vec<_> = split.split(&title).collect();
    while parts.len() > 1 && removable(parts.last().unwrap()) {
        parts.pop();
    }
    let title = parts.join(" - ");
    let feat = regex::Regex::new(r"(?i)\s+(?:feat\.?|ft\.?|featuring)\s+.+$").unwrap();
    feat.replace(&title, "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn artist_matches(candidate: &str, artist: &str) -> bool {
    let norm = normalize(artist);
    normalize(candidate) == norm
        || regex::Regex::new(r"(?i)\s*(?:;|,|\bfeat\.? |\bft\.? |\bfeaturing )\s*")
            .unwrap()
            .split(candidate)
            .any(|s| normalize(s) == norm)
}
pub fn timed_lines(value: &str) -> Value {
    let stamp = regex::Regex::new(r"\[(\d+):([0-5]\d)(?:[.:](\d{1,3}))?\]").unwrap();
    let offsets = regex::Regex::new(r"(?i)\[offset:([+-]?\d+)\]").unwrap();
    let offset = offsets
        .captures_iter(value)
        .last()
        .and_then(|c| c[1].parse::<f64>().ok())
        .unwrap_or(0.)
        / 1000.;
    let words = regex::Regex::new(r"<\d+:[0-5]\d(?:[.:]\d{1,3})?>").unwrap();
    let mut rows = BTreeMap::<i64, Vec<String>>::new();
    for raw in value.lines() {
        let tags: Vec<_> = stamp.captures_iter(raw).collect();
        if tags.is_empty() || !raw[..tags[0].get(0).unwrap().start()].trim().is_empty() {
            continue;
        }
        let text = words
            .replace_all(raw[tags.last().unwrap().get(0).unwrap().end()..].trim(), "")
            .to_string();
        for t in tags {
            let fraction = t
                .get(3)
                .and_then(|v| format!("0.{}", v.as_str()).parse::<f64>().ok())
                .unwrap_or(0.);
            let time = (t[1].parse::<f64>().unwrap_or(86401.) * 60.
                + t[2].parse::<f64>().unwrap_or(0.)
                + fraction
                - offset)
                .max(0.);
            if time > 86400. {
                continue;
            }
            let row = rows
                .entry((time * 1000.).round_ties_even() as i64)
                .or_default();
            if !text.is_empty() && !row.contains(&text) {
                row.push(text.clone());
            }
        }
    }
    json!(rows
        .iter()
        .map(|(t, v)| json!({"time":*t as f64/1000.,"text":v.join("\n")}))
        .collect::<Vec<_>>())
}
pub fn parse(data: &Value) -> Result<Value> {
    ensure!(data.is_object(), "Invalid lyrics response");
    if data["instrumental"] == true {
        return Ok(json!({"status":"instrumental","text":""}));
    }
    let mut plain = s(&data["plainLyrics"]).trim().to_owned();
    if plain.is_empty() {
        plain = regex::Regex::new(r"\[\d+:\d{2}(?:[.:]\d+)?\]")?
            .replace_all(s(&data["syncedLyrics"]), "")
            .into_owned();
        plain =
            regex::Regex::new(r"(?m)^\[(?:ar|al|ti|au|length|by|offset|re|ve):[^\]\r\n]*\]\s*")?
                .replace_all(&plain, "")
                .trim()
                .into();
    }
    let lines = timed_lines(s(&data["syncedLyrics"]));
    if plain.is_empty() && arr(&lines).iter().any(|r| truth(&r["text"])) {
        plain = arr(&lines)
            .iter()
            .map(|r| s(&r["text"]))
            .collect::<Vec<_>>()
            .join("\n");
    }
    let mut r = json!({"status":if plain.is_empty(){"missing"}else{"ready"},"text":plain});
    if !plain.is_empty() && arr(&lines).iter().any(|r| truth(&r["text"])) {
        r["lines"] = lines;
    }
    Ok(r)
}
fn key(track: &Value) -> Result<Value> {
    ensure!(track.is_object(), "Invalid track");
    let mut fields = vec![];
    for k in ["artist", "title", "album"] {
        let value = track.get(k).unwrap_or(&Value::Null);
        let value = if value.is_null() {
            ""
        } else {
            value.as_str().context("Invalid track metadata")?
        };
        ensure!(value.chars().count() <= 1024, "Invalid track metadata");
        fields.push(json!(value.trim()));
    }
    let d = num(&track["duration"]);
    fields.push(json!(if d > 0. && d < 86400. {
        d.round_ties_even()
    } else {
        0.
    }));
    Ok(json!(fields))
}
pub fn track_result(record: &Value, key: &Value) -> Result<Value> {
    let mut result = parse(record)?;
    if result.get("lines").is_some() {
        let length = record["duration"].as_f64();
        let same = num(&key[3]) > 0.
            && length.is_some_and(|l| (l - num(&key[3])).abs() <= 2.)
            && record["trackName"].is_string()
            && normalize(&clean_title(s(&record["trackName"]), true))
                == normalize(&clean_title(s(&key[1]), true))
            && record["artistName"].is_string()
            && artist_matches(s(&record["artistName"]), s(&key[0]));
        if !same {
            result.as_object_mut().unwrap().remove("lines");
        }
    }
    Ok(result)
}
pub fn best(records: &Value, key: &Value) -> Result<Option<Value>> {
    ensure!(records.is_array(), "Invalid lyrics search response");
    let mut matches = vec![];
    for r in arr(records) {
        if !r["trackName"].is_string()
            || !r["artistName"].is_string()
            || normalize(&clean_title(s(&r["trackName"]), false))
                != normalize(&clean_title(s(&key[1]), false))
            || !artist_matches(s(&r["artistName"]), s(&key[0]))
        {
            continue;
        }
        let result = track_result(r, key)?;
        if result["status"] != "ready" {
            continue;
        }
        let diff = if num(&key[3]) > 0. {
            r["duration"]
                .as_f64()
                .map(|n| (n - num(&key[3])).abs())
                .unwrap_or(86400.)
        } else {
            86400.
        };
        let score = (
            truth(&result["lines"]),
            normalize(s(&r["trackName"])) == normalize(s(&key[1])),
            normalize(s(&r["albumName"])) == normalize(s(&key[2])),
            -diff,
        );
        matches.push((score, result));
    }
    matches.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(matches.pop().map(|(_, r)| r))
}
async fn request(url: &str) -> Result<Option<Value>> {
    match tokio::time::timeout(
        std::time::Duration::from_secs(4),
        http(
            url,
            &[
                (
                    "User-Agent".into(),
                    "bartjik/0.1 (desktop lyrics panel)".into(),
                ),
                ("Accept".into(), "application/json".into()),
            ],
            512 * 1024,
        ),
    )
    .await?
    {
        Ok(data) => Ok(Some(serde_json::from_slice(&data)?)),
        Err(e) if e.to_string() == "HTTP 404" => Ok(None),
        Err(e) => Err(e),
    }
}
async fn fallback(key: &Value, mut failed: bool) -> Result<Value> {
    let mut url = url::Url::parse("https://lrclib.net/api/get")?;
    url.query_pairs_mut()
        .append_pair("artist_name", s(&key[0]))
        .append_pair("track_name", s(&key[1]));
    if truth(&key[2]) {
        url.query_pairs_mut().append_pair("album_name", s(&key[2]));
    }
    if num(&key[3]) > 0. {
        url.query_pairs_mut()
            .append_pair("duration", &num(&key[3]).to_string());
    }
    match request(url.as_str()).await {
        Ok(Some(data)) => match track_result(&data, key) {
            Ok(r) if r["status"] == "ready" || r["status"] == "instrumental" => return Ok(r),
            Ok(_) => {}
            Err(_) => failed = true,
        },
        Err(_) => failed = true,
        _ => {}
    }
    let cleaned = clean_title(s(&key[1]), false);
    let mut titles = vec![s(&key[1])];
    if cleaned != s(&key[1]) {
        titles.push(&cleaned);
    }
    for title in titles {
        let mut url = url::Url::parse("https://lrclib.net/api/search")?;
        url.query_pairs_mut()
            .append_pair("artist_name", s(&key[0]))
            .append_pair("track_name", title);
        match request(url.as_str()).await {
            Ok(Some(data)) => match best(&data, key) {
                Ok(Some(r)) => return Ok(r),
                Err(_) => failed = true,
                _ => {}
            },
            Err(_) => failed = true,
            _ => {}
        }
    }
    let mut url = url::Url::parse("https://api.lyrics.ovh/v1/")?;
    url.path_segments_mut()
        .unwrap()
        .pop_if_empty()
        .push(s(&key[0]))
        .push(&cleaned);
    match request(url.as_str()).await {
        Ok(Some(data)) => {
            if let Some(text) = data["lyrics"].as_str() {
                if !text.trim().is_empty() {
                    return Ok(json!({"status":"ready","text":text.trim(),"source":"lyrics.ovh"}));
                }
            } else {
                failed = true;
            }
        }
        Err(_) => failed = true,
        _ => {}
    }
    Ok(json!({"status":if failed{"error"}else{"missing"},"text":""}))
}
pub struct Lyrics {
    spotify: tokio::sync::Mutex<spotify::Spotify>,
    slots: tokio::sync::Semaphore,
    entries: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<(f64, Value)>>>>,
}
impl Lyrics {
    pub fn new() -> Self {
        Self {
            spotify: Default::default(),
            slots: tokio::sync::Semaphore::new(2),
            entries: Default::default(),
        }
    }
    pub async fn get(&self, track: &Value) -> Result<Value> {
        let base = key(track)?;
        if !truth(&base[0]) || !truth(&base[1]) {
            return Ok(json!({"status":"missing","text":""}));
        }
        let id = if regex::Regex::new(r"^[A-Za-z0-9]{22}$")?.is_match(s(&track["spotifyId"])) {
            s(&track["spotifyId"])
        } else {
            ""
        };
        let cookie = if id.is_empty() {
            String::new()
        } else {
            spotify::cookie()
        };
        let fingerprint = format!("{:x}", Sha256::digest(cookie.as_bytes()));
        let key = json!([base, id, fingerprint]).to_string();
        let lock = {
            let mut entries = self.entries.lock().await;
            if entries.len() >= 128 {
                let remove = entries
                    .iter()
                    .filter(|(_, v)| Arc::strong_count(v) == 1)
                    .min_by_key(|(_, v)| v.try_lock().map(|v| v.0 as u64).unwrap_or(u64::MAX))
                    .map(|(k, _)| k.clone());
                if let Some(k) = remove {
                    entries.remove(&k);
                }
            }
            entries.entry(key).or_default().clone()
        };
        let mut cached = lock.lock().await;
        if cached.0 > now() {
            return Ok(cached.1.clone());
        }
        let _slot = self.slots.acquire().await?;
        let mut failed = false;
        let mut result = None;
        if !id.is_empty() && !cookie.is_empty() {
            match self.spotify.lock().await.get(id, &cookie).await {
                Ok(r) => result = r,
                Err(_) => failed = true,
            }
        }
        let r = if let Some(r) = result {
            r
        } else {
            tokio::time::timeout(std::time::Duration::from_secs(17), fallback(&base, failed))
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Lyrics lookup timed out")))
                .unwrap_or(json!({"status":"error","text":""}))
        };
        let mut ttl: f64 = match s(&r["status"]) {
            "error" => 30.,
            "missing" => 900.,
            _ => 21600.,
        };
        if failed {
            ttl = ttl.min(30.);
        }
        *cached = (now() + ttl, r.clone());
        Ok(r)
    }
}
