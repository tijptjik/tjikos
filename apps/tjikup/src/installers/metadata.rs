//! Revalidated GitHub JSON metadata. A cached answer is never an offline success.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const MAX_JSON: u64 = 16 * 1024 * 1024;
const MAX_HEADERS: u64 = 64 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    schema: u8,
    url: String,
    etag: String,
    sha256: String,
    body: Value,
}

struct Response {
    status: u16,
    etag: Option<String>,
    body: Vec<u8>,
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_etag(value: &str) -> bool {
    let quoted = value.strip_prefix("W/").unwrap_or(value);
    value.len() <= 4096
        && quoted.starts_with('"')
        && quoted.ends_with('"')
        && quoted.len() >= 2
        && quoted[1..quoted.len() - 1]
            .bytes()
            .all(|b| b == 0x21 || (0x23..=0x7e).contains(&b))
}

fn cache_path(url: &str) -> Result<PathBuf> {
    let root = std::env::var_os("XDG_CACHE_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or(crate::commands::home()?.join(".cache"))
        .join("tjikup/releases");
    let mut current = PathBuf::new();
    for component in root.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.is_dir() => (),
            Ok(_) => bail!("release cache path contains a non-directory"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                match fs::DirBuilder::new().mode(0o700).create(&current) {
                    Ok(()) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        if !fs::symlink_metadata(&current)?.is_dir() {
                            bail!("release cache path contains a non-directory");
                        }
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
    let meta = fs::metadata(&root)?;
    if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        bail!("release cache must be private and owned by this user");
    }
    Ok(root.join(format!("{}.json", hash(url.as_bytes()))))
}

fn read_entry(path: &Path, url: &str) -> Option<Entry> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
        || meta.len() > MAX_JSON + 8192
    {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_JSON + 8193).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_JSON + 8192 {
        return None;
    }
    let entry: Entry = serde_json::from_slice(&bytes).ok()?;
    (entry.schema == 1
        && entry.url == url
        && valid_etag(&entry.etag)
        && (entry.body.is_object() || entry.body.is_array())
        && entry.sha256 == hash(&serde_json::to_vec(&entry.body).ok()?))
    .then_some(entry)
}

fn write_entry(path: &Path, url: &str, etag: &str, body: &Value) -> Result<()> {
    let entry = Entry {
        schema: 1,
        url: url.to_owned(),
        etag: etag.to_owned(),
        sha256: hash(&serde_json::to_vec(body)?),
        body: body.clone(),
    };
    let mut stage = tempfile::NamedTempFile::new_in(path.parent().context("cache parent")?)?;
    serde_json::to_writer(&mut stage, &entry)?;
    stage.flush()?;
    stage.as_file().sync_all()?;
    // A complete body and its matching ETag are published in one rename. Readers
    // retain their own snapshot even when another process refreshes this URL.
    stage.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn parse_headers(bytes: &[u8]) -> Result<(u16, Option<String>)> {
    let headers = std::str::from_utf8(bytes).context("invalid GitHub response headers")?;
    let mut status = None;
    let mut etag = None;
    for line in headers.lines() {
        if line.starts_with("HTTP/") {
            status = line.split_whitespace().nth(1).and_then(|s| s.parse().ok());
            etag = None;
        } else if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("etag") && valid_etag(value.trim()) {
                etag = Some(value.trim().to_owned());
            }
        }
    }
    Ok((status.context("GitHub response status missing")?, etag))
}

fn request(url: &str, token: Option<&str>, etag: Option<&str>, curl: &Path) -> Result<Response> {
    let headers = tempfile::NamedTempFile::new()?;
    let stderr = tempfile::NamedTempFile::new()?;
    let mut request_headers = tempfile::NamedTempFile::new()?;
    if let Some(token) = token {
        if token.contains(['\r', '\n']) {
            bail!("invalid GitHub credential");
        }
        writeln!(request_headers, "Authorization: Bearer {token}")?;
    }
    if let Some(etag) = etag {
        if !valid_etag(etag) {
            bail!("invalid cached GitHub ETag");
        }
        writeln!(request_headers, "If-None-Match: {etag}")?;
    }
    request_headers.flush()?;
    let mut child = Command::new(curl)
        .args([
            "--proto",
            "=https",
            "--silent",
            "--show-error",
            "--fail",
            "--max-time",
            "60",
            "--max-filesize",
            &MAX_JSON.to_string(),
            "--user-agent",
            "tjikup-installer",
            "--header",
            "Accept: application/vnd.github+json",
            "--header",
            "X-GitHub-Api-Version: 2022-11-28",
            "--header",
            &format!("@{}", request_headers.path().display()),
            "--dump-header",
        ])
        .arg(headers.path())
        // Metadata redirects are never followed; credentials stay at api.github.com.
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(stderr.reopen()?)
        .spawn()?;
    let mut body = Vec::new();
    let read = child
        .stdout
        .take()
        .context("metadata stdout missing")?
        .take(MAX_JSON + 1)
        .read_to_end(&mut body);
    if read.is_err() || body.len() as u64 > MAX_JSON {
        let _ = child.kill();
        let _ = child.wait();
        read?;
        bail!("GitHub metadata exceeds size limit");
    }
    if !child.wait()?.success() {
        bail!("GitHub metadata lookup failed; check network access and GitHub authentication");
    }
    let mut response_headers = Vec::new();
    headers
        .reopen()?
        .take(MAX_HEADERS + 1)
        .read_to_end(&mut response_headers)?;
    if response_headers.len() as u64 > MAX_HEADERS {
        bail!("GitHub response headers exceed size limit");
    }
    let (status, etag) = parse_headers(&response_headers)?;
    Ok(Response { status, etag, body })
}

fn fetch_cached(
    url: &str,
    path: &Path,
    mut get: impl FnMut(Option<&str>) -> Result<Response>,
) -> Result<Value> {
    let saved = read_entry(path, url);
    let mut response = get(saved.as_ref().map(|entry| entry.etag.as_str()))?;
    if response.status == 304 {
        if let Some(saved) = saved {
            return Ok(saved.body);
        }
        // Do not mistake an orphaned validator or unexpected 304 for current data.
        response = get(None)?;
    }
    if response.status != 200 {
        bail!("GitHub metadata returned HTTP {}", response.status);
    }
    let body: Value =
        serde_json::from_slice(&response.body).context("invalid GitHub release metadata")?;
    if !body.is_array() && !body.is_object() {
        bail!("GitHub metadata must be a JSON object or array");
    }
    if let Some(etag) = response.etag.filter(|etag| valid_etag(etag)) {
        write_entry(path, url, &etag, &body)?;
    }
    Ok(body)
}

pub(super) fn fetch(url: &str, token: Option<String>) -> Result<Value> {
    if !url.starts_with("https://api.github.com/repos/") {
        bail!("unexpected metadata origin");
    }
    let path = cache_path(url)?;
    fetch_cached(url, &path, |etag| {
        request(url, token.as_deref(), etag, Path::new("curl"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    const URL: &str = "https://api.github.com/repos/owner/tool/releases/latest";

    fn response(status: u16, etag: Option<&str>, body: Value) -> Response {
        Response {
            status,
            etag: etag.map(str::to_owned),
            body: serde_json::to_vec(&body).unwrap(),
        }
    }

    #[test]
    fn cache_revalidates_and_replaces_body_and_validator_together() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("release.json");
        let first = json!({"tag_name":"v1.2.3"});
        assert_eq!(
            fetch_cached(URL, &path, |tag| {
                assert!(tag.is_none());
                Ok(response(200, Some("\"first\""), first.clone()))
            })
            .unwrap(),
            first
        );
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            fetch_cached(URL, &path, |tag| {
                assert_eq!(tag, Some("\"first\""));
                Ok(response(304, None, Value::Null))
            })
            .unwrap(),
            first
        );
        let next = json!({"tag_name":"v1.3.0"});
        assert_eq!(
            fetch_cached(URL, &path, |_| Ok(response(
                200,
                Some("W/\"next\""),
                next.clone()
            )))
            .unwrap(),
            next
        );
        assert_eq!(read_entry(&path, URL).unwrap().etag, "W/\"next\"");
    }

    #[test]
    fn corrupt_cache_and_unexpected_304_retry_without_validator() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("release.json");
        write_entry(&path, URL, "\"saved\"", &json!({"tag_name":"v1.2.3"})).unwrap();
        let mut entry: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        entry["body"]["tag_name"] = json!("v0.0.0");
        fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
        let mut calls = 0;
        let body = fetch_cached(URL, &path, |etag| {
            calls += 1;
            assert!(etag.is_none());
            Ok(response(
                if calls == 1 { 304 } else { 200 },
                Some("\"fresh\""),
                json!({"tag_name":"v2.0.0"}),
            ))
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(body["tag_name"], "v2.0.0");
        assert!(read_entry(
            &path,
            "https://api.github.com/repos/other/tool/releases/latest"
        )
        .is_none());
    }

    #[test]
    fn network_and_invalid_json_errors_never_return_stale_success() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("release.json");
        write_entry(&path, URL, "\"saved\"", &json!({"tag_name":"v1.2.3"})).unwrap();
        assert!(fetch_cached(URL, &path, |_| bail!("network unavailable")).is_err());
        assert!(fetch_cached(URL, &path, |_| Ok(Response {
            status: 200,
            etag: Some("\"bad\"".into()),
            body: b"broken".to_vec()
        }))
        .is_err());
        assert!(fetch_cached(URL, &path, |_| Ok(response(
            403,
            None,
            json!({"error":"rate limit"})
        )))
        .is_err());
        assert_eq!(read_entry(&path, URL).unwrap().etag, "\"saved\"");
    }

    #[test]
    fn no_body_after_two_304s_and_non_json_documents_fail() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("release.json");
        let mut calls = 0;
        assert!(fetch_cached(URL, &path, |_| {
            calls += 1;
            Ok(response(304, None, Value::Null))
        })
        .is_err());
        assert_eq!(calls, 2);
        assert!(fetch_cached(URL, &path, |_| Ok(response(
            200,
            Some("\"x\""),
            json!("unexpected")
        )))
        .is_err());
        assert!(!path.exists());
    }

    #[test]
    fn headers_use_final_status_and_reject_unsafe_validators() {
        let (status, tag) = parse_headers(
            b"HTTP/1.1 200 Connection established\r\n\r\nHTTP/2 304\r\nETag: W/\"new\"\r\n\r\n",
        )
        .unwrap();
        assert_eq!(status, 304);
        assert_eq!(tag.as_deref(), Some("W/\"new\""));
        for bad in [
            "",
            "unquoted",
            "\"a\"\r\nOther: value",
            "\"with space\"",
            "\"a\"b\"",
        ] {
            assert!(!valid_etag(bad));
        }
    }

    #[test]
    fn curl_keeps_headers_private_and_does_not_follow_redirects() {
        let temp = tempfile::tempdir().unwrap();
        let curl = temp.path().join("curl");
        fs::write(
            &curl,
            r#"#!/bin/sh
for arg; do
    [ "$arg" != --location ] || exit 8
    [ "$arg" != -L ] || exit 9
    if [ "$dump" = yes ]; then
        printf 'HTTP/2 200\r\nETag: "fresh"\r\n\r\n' > "$arg"
        dump=no
    fi
    if [ "$header" = yes ]; then
        case "$arg" in @*)
            file=${arg#@}
            [ "$(/usr/bin/stat -c %a "$file")" = 600 ] || exit 10
            /usr/bin/grep -q 'Authorization: Bearer synthetic-token' "$file" || exit 11
            /usr/bin/grep -q 'If-None-Match: "saved"' "$file" || exit 12
        esac
        header=no
    fi
    [ "$arg" != --dump-header ] || dump=yes
    [ "$arg" != --header ] || header=yes
done
printf '{"tag_name":"v1.2.3"}'
"#,
        )
        .unwrap();
        fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).unwrap();
        let answer = request(URL, Some("synthetic-token"), Some("\"saved\""), &curl).unwrap();
        assert_eq!(answer.status, 200);
        assert_eq!(answer.etag.as_deref(), Some("\"fresh\""));
        assert!(request(URL, Some("bad\r\ncredential"), None, &curl).is_err());
    }
}
