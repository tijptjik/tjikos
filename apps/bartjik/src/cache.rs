use crate::util::*;
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc};
pub fn raster(data: &[u8]) -> bool {
    data.starts_with(b"\x89PNG\r\n\x1a\n")
        || data.starts_with(b"\xff\xd8\xff")
        || data.starts_with(b"GIF87a")
        || data.starts_with(b"GIF89a")
        || (data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP"))
}
pub struct Artwork {
    slots: tokio::sync::Semaphore,
    locks: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<f64>>>>,
}
impl Artwork {
    pub fn new() -> Self {
        Self {
            slots: tokio::sync::Semaphore::new(2),
            locks: Default::default(),
        }
    }
    pub async fn get(&self, url: &str) -> Result<String> {
        ensure!(url.len() <= 8192, "Invalid artwork URL");
        let u = url::Url::parse(url)?;
        ensure!(
            ["http", "https"].contains(&u.scheme())
                && u.host().is_some()
                && u.username().is_empty()
                && u.password().is_none(),
            "Artwork requires an HTTP(S) URL"
        );
        let key = format!("{:x}", Sha256::digest(url.as_bytes()));
        let lock = {
            let mut locks = self.locks.lock().await;
            if locks.len() >= 256 {
                locks.retain(|_, v| Arc::strong_count(v) > 1);
            }
            locks.entry(key.clone()).or_default().clone()
        };
        let mut retry = lock.lock().await;
        ensure!(*retry <= now(), "Artwork retry cooling down");
        let _slot = self.slots.acquire().await?;
        let dir = xdg("XDG_CACHE_HOME", ".cache").join("bartjik/artwork");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{key}.img"));
        if let Ok(m) = path.metadata() {
            if m.len() > 0 && m.len() <= 8 * 1024 * 1024 {
                use std::io::Read;
                let mut header = [0; 16];
                if std::fs::File::open(&path)?.read(&mut header).is_ok() && raster(&header) {
                    std::fs::File::open(&path)?.set_modified(std::time::SystemTime::now())?;
                    return Ok(url::Url::from_file_path(path).unwrap().to_string());
                }
            }
        }
        let result = async {
            let data = http(
                url,
                &[
                    ("User-Agent".into(), "bartjik/0.1".into()),
                    ("Accept".into(), "image/*".into()),
                ],
                8 * 1024 * 1024,
            )
            .await?;
            ensure!(raster(&data), "Unsupported artwork image");
            use std::io::Write;
            let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
            temp.write_all(&data)?;
            temp.persist(&path)?;
            let mut entries: Vec<_> = std::fs::read_dir(&dir)?
                .flatten()
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x == "img") {
                        let m = e.metadata().ok()?;
                        Some((m.modified().ok()?, m.len(), p))
                    } else {
                        None
                    }
                })
                .collect();
            entries.sort();
            let mut total = entries.iter().map(|x| x.1).sum::<u64>();
            let mut count = entries.len();
            for (_, size, p) in entries {
                if total <= 128 * 1024 * 1024 && count <= 256 {
                    break;
                }
                if p != path && std::fs::remove_file(p).is_ok() {
                    total -= size;
                    count -= 1;
                }
            }
            Ok(url::Url::from_file_path(path).unwrap().to_string())
        }
        .await;
        if result.is_err() {
            *retry = now() + 60.;
        }
        result
    }
}
pub fn normalize(data: &Value) -> Result<Value> {
    let rows = arr(&data["prices"]);
    ensure!((2..=2048).contains(&rows.len()), "Invalid price history");
    let mut points = vec![];
    for r in rows {
        ensure!(
            r.is_array()
                && arr(r).len() == 2
                && r[0].as_f64().is_some_and(|v| v > 0.)
                && r[1].as_f64().is_some_and(|v| v >= 0.),
            "Invalid history value"
        );
        if let Some(old) = points.iter_mut().find(|p: &&mut Value| p[0] == r[0]) {
            *old = r.clone();
        } else {
            points.push(r.clone());
        }
    }
    points.sort_by(|a, b| num(&a[0]).total_cmp(&num(&b[0])));
    ensure!(points.len() >= 2, "Insufficient price history");
    let end = num(&points.last().unwrap()[0]);
    points.retain(|p| num(&p[0]) >= end - 86400000.);
    Ok(json!(points))
}
pub struct History {
    entries: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<Value>>>>,
    slots: tokio::sync::Semaphore,
    path: std::path::PathBuf,
}
impl History {
    pub fn new() -> Self {
        let path = xdg("XDG_CACHE_HOME", ".cache").join("bartjik/crypto-history.json");
        let mut entries = HashMap::new();
        if path.metadata().is_ok_and(|m| m.len() <= 4000000) {
            if let Ok(rows) = read(&path) {
                for r in arr(&rows).iter().rev().take(64) {
                    let key = &r["key"];
                    if arr(key).len() != 2
                        || !arr(key).iter().all(Value::is_string)
                        || num(&r["attempted"]) > now() + 60.
                    {
                        continue;
                    }
                    let mut r = r.clone();
                    if truth(&r["chart"]) {
                        if normalize(&json!({"prices":r["chart"]["points"]})).is_err()
                            || num(&r["chart"]["updated"]) > now() + 60.
                        {
                            continue;
                        }
                        r["chart"]["source"] = json!("CoinGecko");
                    }
                    entries.insert(key.to_string(), Arc::new(tokio::sync::Mutex::new(r)));
                }
            }
        }
        Self {
            entries: tokio::sync::Mutex::new(entries),
            slots: tokio::sync::Semaphore::new(8),
            path,
        }
    }
    pub async fn get(&self, coin: &str, currency: &str) -> Result<Value> {
        ensure!(
            regex::Regex::new("^[a-z0-9-]{1,80}$")?.is_match(coin)
                && regex::Regex::new("^[a-z]{2,10}$")?.is_match(currency),
            "Configure a valid chartId for this asset"
        );
        let key = json!([coin, currency]);
        let lock = {
            let mut entries = self.entries.lock().await;
            if entries.len() >= 64 {
                let remove = entries
                    .iter()
                    .filter(|(_, v)| Arc::strong_count(v) == 1)
                    .min_by(|(a, _), (b, _)| a.cmp(b))
                    .map(|(k, _)| k.clone());
                if let Some(k) = remove {
                    entries.remove(&k);
                }
            }
            entries
                .entry(key.to_string())
                .or_insert_with(|| {
                    Arc::new(tokio::sync::Mutex::new(json!({"key":key,"attempted":0})))
                })
                .clone()
        };
        let mut entry = lock.lock().await;
        let cached = entry["chart"].clone();
        if truth(&cached) && now() - num(&cached["updated"]) < 60. {
            return Ok(cached);
        }
        if now() < num(&entry["retry"][0]) {
            return Ok(crate::config::merge(
                &cached,
                &json!({"error":entry["retry"][1],"stale":truth(&cached)}),
            ));
        }
        if now() - num(&entry["attempted"]) < 60. {
            return Ok(crate::config::merge(
                &cached,
                &json!({"error":if truth(&cached){""}else{"History request cooling down"},"stale":truth(&cached)}),
            ));
        }
        let _slot = self
            .slots
            .try_acquire()
            .map_err(|_| anyhow::anyhow!("History request queue full"))?;
        entry["attempted"] = json!(now());
        let result=hj(&format!("https://api.coingecko.com/api/v3/coins/{coin}/market_chart?vs_currency={currency}&days=1"),&[("Accept".into(),"application/json".into()),("User-Agent".into(),"bartjik/0.1".into())]).await.and_then(|v|normalize(&v));
        let result = match result {
            Ok(points) => {
                let chart = json!({"points":points,"updated":now(),"source":"CoinGecko","error":"","stale":false});
                entry["chart"] = chart.clone();
                entry["retry"] = Value::Null;
                chart
            }
            Err(e) => {
                let text = e.to_string();
                let delay = text
                    .strip_prefix("HTTP 429; retry ")
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(60.)
                    .max(60.);
                entry["retry"] = json!([now() + delay, text]);
                crate::config::merge(&cached, &json!({"error":text,"stale":truth(&cached)}))
            }
        };
        drop(entry);
        let locks = self
            .entries
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut rows = vec![];
        for l in locks {
            if let Ok(v) = l.try_lock() {
                rows.push(v.clone());
            }
        }
        let _ = save(&self.path, &json!(rows));
        Ok(result)
    }
}
