//! Origin-only, bounded favicon fetching with a private persistent cache.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Cursor, Read},
    net::ToSocketAddrs,
    time::Duration,
};

pub fn request(link: &str) -> Option<crate::contacts::PhotoRequest> {
    let url = url::Url::parse(link).ok()?;
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let host = url.host_str()?;
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if !crate::google::public_ip(ip) {
            return None;
        }
    }
    let icon = format!("https://{host}/favicon.ico");
    Some(crate::contacts::PhotoRequest {
        key: format!("favicon:{icon}"),
        urls: vec![icon],
    })
}

#[derive(Serialize, Deserialize)]
struct Entry {
    at: u64,
    bytes: Vec<u8>,
}
impl Entry {
    fn fresh(&self, now: u64) -> bool {
        now.saturating_sub(self.at)
            < if self.bytes.is_empty() {
                86400
            } else {
                7 * 86400
            }
    }
}

pub fn cached(url: &str) -> Vec<u8> {
    let path = crate::store::data_dir().ok().and_then(|dir| {
        let dir = dir.join("favicons");
        crate::store::private_dir(&dir).ok()?;
        Some(dir.join(format!("{:x}.json", Sha256::digest(url.as_bytes()))))
    });
    cached_at(path.as_deref(), crate::auth::now(), || {
        fetch(url).unwrap_or_default()
    })
}

fn cached_at(path: Option<&std::path::Path>, now: u64, fetch: impl FnOnce() -> Vec<u8>) -> Vec<u8> {
    if let Some(entry) = path
        .as_ref()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Entry>(&bytes).ok())
        .filter(|entry| entry.fresh(now))
    {
        return entry.bytes;
    }
    let bytes = fetch();
    if let Some(path) = path {
        let entry = Entry {
            at: now,
            bytes: bytes.clone(),
        };
        if let Ok(json) = serde_json::to_vec(&entry) {
            let _ = crate::store::write_private(&path, &json);
        }
    }
    bytes
}

fn fetch(url: &str) -> Result<Vec<u8>> {
    let parsed = url::Url::parse(url)?;
    ensure!(
        request(url).is_some_and(|request| request.urls[0] == url),
        "Invalid favicon URL"
    );
    let host = parsed.host_str().context("Missing host")?;
    let addresses: Vec<_> = (host, 443).to_socket_addrs()?.collect();
    ensure!(
        !addresses.is_empty()
            && addresses
                .iter()
                .all(|address| crate::google::public_ip(address.ip())),
        "Non-public favicon host"
    );
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(4))
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &addresses)
        .no_proxy()
        .build()?
        .get(url)
        .send()?
        .error_for_status()?;
    let mut bytes = Vec::new();
    response.take(256_001).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 256_000, "Favicon too large");
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(512);
    limits.max_image_height = Some(512);
    reader.limits(limits);
    let image = reader.decode()?.thumbnail(32, 32);
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png)?;
    Ok(png.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistent_success_and_failure_entries_avoid_refetch_until_expiry() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let path = dir.path().join("icon.json");
        assert_eq!(cached_at(Some(&path), 10, || vec![1, 2, 3]), vec![1, 2, 3]);
        assert_eq!(
            cached_at(Some(&path), 11, || panic!("must use disk cache")),
            vec![1, 2, 3]
        );
        assert!(cached_at(Some(&path), 604810, Vec::new).is_empty());
        assert!(cached_at(Some(&path), 604811, || panic!("must cache failures too")).is_empty());
        assert_eq!(cached_at(Some(&path), 691210, || vec![4]), vec![4]);
    }

    #[test]
    fn requests_share_origins_and_never_include_message_paths() {
        let a = request("https://example.test/private/path?token=secret").unwrap();
        let b = request("https://example.test/other").unwrap();
        assert_eq!(a.key, b.key);
        assert_eq!(a.urls, vec!["https://example.test/favicon.ico"]);
        for url in [
            "mailto:a@example.test",
            "file:///tmp/icon",
            "https://127.0.0.1/",
            "https://user:secret@example.test/",
            "https://example.test:8443/",
        ] {
            assert!(request(url).is_none(), "{url}");
        }
    }
    #[test]
    fn failures_have_a_shorter_persistent_cache_lifetime() {
        assert!(
            Entry {
                at: 1,
                bytes: vec![]
            }
            .fresh(86400)
        );
        assert!(
            !Entry {
                at: 1,
                bytes: vec![]
            }
            .fresh(86401)
        );
        assert!(
            Entry {
                at: 1,
                bytes: vec![1]
            }
            .fresh(86401)
        );
        assert!(
            !Entry {
                at: 1,
                bytes: vec![1]
            }
            .fresh(604801)
        );
    }
}
