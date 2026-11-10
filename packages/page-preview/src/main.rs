use anyhow::{ensure, Context, Result};
use reqwest::blocking::Client;
use scraper::{Html, Selector};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{self, Read, Write},
    os::unix::{fs::DirBuilderExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime},
};
use url::Url;

#[derive(Default)]
struct Metadata {
    values: HashMap<String, String>,
    icons: Vec<String>,
    title: String,
}
fn metadata(text: &str) -> Metadata {
    let html = Html::parse_document(text);
    let mut result = Metadata::default();
    for node in html.select(&Selector::parse("meta").unwrap()) {
        let attrs = node.value();
        let key = attrs
            .attr("property")
            .or_else(|| attrs.attr("name"))
            .unwrap_or("")
            .to_lowercase();
        result
            .values
            .entry(key)
            .or_insert_with(|| attrs.attr("content").unwrap_or("").trim().into());
    }
    for node in html.select(&Selector::parse("link").unwrap()) {
        if node
            .value()
            .attr("rel")
            .unwrap_or("")
            .split_whitespace()
            .any(|r| r.eq_ignore_ascii_case("icon") || r.eq_ignore_ascii_case("apple-touch-icon"))
        {
            if let Some(href) = node.value().attr("href") {
                result.icons.push(href.into());
            }
        }
    }
    for node in html.select(&Selector::parse("title").unwrap()) {
        result.title.extend(node.text());
    }
    result
}
fn render_failure(text: &str) -> &'static str {
    let title = metadata(text).title.trim().to_lowercase();
    if [
        "403",
        "access denied",
        "request blocked",
        "request could not be satisfied",
        "just a moment",
        "attention required",
    ]
    .iter()
    .any(|word| title.contains(word))
    {
        "blocked"
    } else if ["sign in", "log in", "login"]
        .iter()
        .any(|word| title.contains(word))
    {
        "sign-in"
    } else if text.contains("class=\"neterror\"") || text.contains("id=\"main-frame-error\"") {
        "failed"
    } else {
        ""
    }
}
fn web_url(text: &str) -> Result<Url> {
    let url = Url::parse(text)?;
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "Only web pages support previews"
    );
    Ok(url)
}
fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().context("Missing cache directory")?)?;
    file.write_all(data)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
fn uri(path: &Path) -> Result<String> {
    Ok(Url::from_file_path(path)
        .map_err(|_| anyhow::anyhow!("Cache path must be absolute"))?
        .into())
}
struct Fetch {
    data: Vec<u8>,
    content_type: String,
    charset: String,
    url: Url,
}
struct Previews {
    cache: PathBuf,
    client: Client,
    cancelled: Arc<AtomicBool>,
}
impl Previews {
    fn new(cache: PathBuf, cancelled: Arc<AtomicBool>) -> Result<Self> {
        let client = Client::builder()
            .user_agent("Tasktjik/1.0 (saved-page preview)")
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if !matches!(attempt.url().scheme(), "http" | "https") {
                    attempt.error("Unsupported redirect")
                } else if attempt.previous().len() >= 10 {
                    attempt.error("Too many redirects")
                } else {
                    attempt.follow()
                }
            }))
            .timeout(Duration::from_secs(5))
            .build()?;
        Ok(Self {
            cache,
            client,
            cancelled,
        })
    }
    fn check(&self) -> Result<()> {
        ensure!(!self.cancelled.load(Ordering::Relaxed), "Preview cancelled");
        Ok(())
    }
    fn paths(&self, url: &str) -> (String, PathBuf) {
        let key = format!("{:x}", Sha256::digest(url.as_bytes()));
        let receipt = self.cache.join(format!("{key}.json"));
        (key, receipt)
    }
    fn cached(&self, url: &str) -> Option<Value> {
        let (_, receipt) = self.paths(url);
        let saved: Value = serde_json::from_slice(&fs::read(&receipt).ok()?).ok()?;
        if !saved.is_object() || saved["previewVersion"] != 4 {
            return None;
        }
        if let Some(image) = saved["image"].as_str().filter(|s| !s.is_empty()) {
            let url = Url::parse(image).ok()?;
            if url.scheme() != "file" || url.host_str().is_some() {
                return None;
            }
            let path = url.to_file_path().ok()?;
            if path.parent() != Some(self.cache.as_path())
                || !path.is_file()
                || fs::metadata(path).ok()?.len() == 0
            {
                return None;
            }
        } else if SystemTime::now()
            .duration_since(fs::metadata(receipt).ok()?.modified().ok()?)
            .unwrap_or_default()
            >= Duration::from_secs(300)
        {
            return None;
        }
        Some(saved)
    }
    fn fetch(&self, url: &str, limit: usize, deadline: Instant) -> Result<Fetch> {
        self.check()?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .context("Preview deadline exceeded")?;
        let url = web_url(url)?;
        let mut response = self
            .client
            .get(url)
            .header("Accept", "text/html,image/*")
            .timeout(remaining.min(Duration::from_secs(5)))
            .send()?
            .error_for_status()?;
        let url = web_url(response.url().as_str())?;
        let header = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let content_type = header.split(';').next().unwrap_or("").trim().to_lowercase();
        let charset = header
            .split(';')
            .skip(1)
            .find_map(|p| {
                let (key, value) = p.trim().split_once('=')?;
                key.eq_ignore_ascii_case("charset")
                    .then(|| value.trim_matches(['\'', '"']).to_owned())
            })
            .unwrap_or_else(|| "utf-8".into());
        let mut data = Vec::new();
        (&mut response)
            .take(limit as u64 + 1)
            .read_to_end(&mut data)?;
        self.check()?;
        ensure!(data.len() <= limit, "Preview too large");
        Ok(Fetch {
            data,
            content_type,
            charset,
            url,
        })
    }
    fn favicon(&self, url: &Url, icons: &[String], key: &str) -> Result<Option<String>> {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut seen = HashSet::new();
        for candidate in icons
            .iter()
            .map(String::as_str)
            .chain(std::iter::once("/favicon.ico"))
            .filter_map(|icon| url.join(icon).ok())
            .filter(|url| seen.insert(url.to_string()))
            .take(3)
        {
            self.check()?;
            if let Ok(fetched) = self.fetch(candidate.as_str(), 512 * 1024, deadline) {
                let extension = match fetched.content_type.as_str() {
                    "image/svg+xml" => Some("svg"),
                    "image/x-icon" | "image/vnd.microsoft.icon" => Some("ico"),
                    kind => image_extension(kind)
                        .or_else(|| fetched.data.starts_with(b"\0\0\x01\0").then_some("ico")),
                };
                if let Some(ext) = extension {
                    let path = self.cache.join(format!("{key}-favicon.{ext}"));
                    atomic_write(&path, &fetched.data)?;
                    return Ok(Some(uri(&path)?));
                }
            }
        }
        Ok(None)
    }
    fn render(&self, url: &str, key: &str) -> Result<Value> {
        self.check()?;
        let browser = [
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
        ]
        .iter()
        .find_map(|name| which(name));
        let Some(browser) = browser else {
            return Ok(json!({"renderError":"browser-unavailable"}));
        };
        let directory = tempfile::Builder::new()
            .prefix("render-")
            .tempdir_in(&self.cache)?;
        let screenshot = directory.path().join("page.png");
        let document = directory.path().join("document.html");
        let mut command = Command::new(browser);
        command
            .args([
                "--headless=new",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--disable-extensions",
                "--disable-sync",
                "--disable-crash-reporter",
                "--mute-audio",
                "--hide-scrollbars",
                "--window-size=1280,800",
                "--virtual-time-budget=7000",
                "--run-all-compositor-stages-before-draw",
                "--dump-dom",
            ])
            .arg(format!(
                "--user-data-dir={}",
                directory.path().join("profile").display()
            ))
            .arg(format!("--screenshot={}", screenshot.display()))
            .arg(url)
            .env("TMPDIR", directory.path())
            .stdout(File::create(&document)?)
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .process_group(0);
        // Drop kills and reaps the entire browser group before its temporary profile.
        let mut process = Browser(command.spawn()?);
        let deadline = Instant::now() + Duration::from_secs(25);
        loop {
            self.check()?;
            if let Some(status) = process.0.try_wait()? {
                if !status.success() || !screenshot.is_file() {
                    return Ok(json!({"renderError":"failed"}));
                }
                break;
            }
            if Instant::now() >= deadline {
                return Ok(json!({"renderError":"timeout"}));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let mut data = Vec::new();
        File::open(document)?
            .take(4 * 1024 * 1024)
            .read_to_end(&mut data)?;
        let failure = render_failure(&String::from_utf8_lossy(&data));
        if !failure.is_empty() {
            return Ok(json!({"renderError":failure}));
        }
        let mut data = Vec::new();
        File::open(screenshot)?
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut data)?;
        if !data.starts_with(b"\x89PNG\r\n\x1a\n") || data.len() > 8 * 1024 * 1024 {
            return Ok(json!({"renderError":"failed"}));
        }
        self.check()?;
        let target = self.cache.join(format!("{key}-page.png"));
        atomic_write(&target, &data)?;
        Ok(json!({"image":uri(&target)?, "imageKind":"page"}))
    }
    fn preview(&self, text: &str) -> Result<Value> {
        self.check()?;
        let Ok(mut resolved) = web_url(text) else {
            return Ok(json!({}));
        };
        if let Some(saved) = self.cached(text) {
            return Ok(saved);
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.cache)?;
        let (key, receipt) = self.paths(text);
        let mut result = json!({});
        let mut icons = vec![];
        let deadline = Instant::now() + Duration::from_secs(12);
        match self.fetch(text, 2 * 1024 * 1024, deadline) {
            Ok(page)
                if matches!(
                    page.content_type.as_str(),
                    "text/html" | "application/xhtml+xml"
                ) =>
            {
                resolved = page.url;
                let encoding = encoding_rs::Encoding::for_label(page.charset.as_bytes())
                    .unwrap_or(encoding_rs::UTF_8);
                let (document, _, _) = encoding.decode(&page.data);
                let meta = metadata(&document);
                let pick = |keys: &[&str]| {
                    keys.iter()
                        .find_map(|key| meta.values.get(*key).filter(|s| !s.is_empty()))
                        .cloned()
                        .unwrap_or_default()
                };
                result["description"] =
                    json!(
                        pick(&["og:description", "description", "twitter:description"])
                            .chars()
                            .take(600)
                            .collect::<String>()
                    );
                let title = pick(&["og:title"]);
                result["title"] = json!(if title.is_empty() {
                    meta.title.trim()
                } else {
                    &title
                }
                .chars()
                .take(300)
                .collect::<String>());
                let image = pick(&["og:image", "twitter:image"]);
                if !image.is_empty() {
                    if let Ok(url) = resolved.join(&image) {
                        if let Ok(image) = self.fetch(url.as_str(), 5 * 1024 * 1024, deadline) {
                            if let Some(ext) = image_extension(&image.content_type) {
                                let path = self.cache.join(format!("{key}.{ext}"));
                                atomic_write(&path, &image.data)?;
                                result["image"] = json!(uri(&path)?);
                                result["imageKind"] = json!("metadata");
                            }
                        }
                    }
                }
                icons = meta.icons;
            }
            _ => {
                result["unavailable"] = json!(true);
            }
        }
        self.check()?;
        if result["unavailable"] != true {
            if let Ok(render) = self.render(text, &key) {
                result
                    .as_object_mut()
                    .unwrap()
                    .extend(render.as_object().unwrap().clone());
            }
        }
        self.check()?;
        if matches!(result["renderError"].as_str(), Some("blocked" | "sign-in"))
            || result["image"].as_str().unwrap_or("").is_empty()
        {
            if let Ok(Some(icon)) = self.favicon(&resolved, &icons, &key) {
                result["image"] = json!(icon);
                result["imageKind"] = json!("favicon");
            }
        }
        self.check()?;
        result["previewVersion"] = json!(4);
        atomic_write(&receipt, &serde_json::to_vec(&result)?)?;
        Ok(result)
    }
    fn batch(&self, urls: Vec<String>, mut emit: impl FnMut(Value) -> Result<()>) -> Result<()> {
        let mut seen = HashSet::new();
        let mut pending = vec![];
        for url in urls.into_iter().filter(|url| seen.insert(url.clone())) {
            self.check()?;
            if let Some(saved) = self.cached(&url) {
                emit(json!({"url":url,"preview":saved}))?;
            } else {
                pending.push(url);
            }
        }
        for url in pending {
            self.check()?;
            let result = self.preview(&url).unwrap_or_else(|_| json!({}));
            self.check()?;
            emit(json!({"url":url,"preview":result}))?;
        }
        Ok(())
    }
}
fn image_extension(kind: &str) -> Option<&'static str> {
    match kind {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}
struct Browser(Child);
impl Drop for Browser {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}
fn which(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join(name))
        .find(|path| {
            path.is_file()
                && fs::metadata(path)
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
        })
}
fn main() {
    let cancelled = Arc::new(AtomicBool::new(false));
    for signal in [libc::SIGTERM, libc::SIGINT] {
        if signal_hook::flag::register(signal, cancelled.clone()).is_err() {
            return;
        }
    }
    let run = || -> Result<()> {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
            });
        let previews = Previews::new(base.join("tasktjik/page-previews"), cancelled.clone())?;
        let mut input = String::new();
        io::stdin()
            .take(1024 * 1024 + 1)
            .read_to_string(&mut input)?;
        ensure!(input.len() <= 1024 * 1024, "Preview request too large");
        let mut output = io::stdout().lock();
        let mut emit = |value: Value| -> Result<()> {
            serde_json::to_writer(&mut output, &value)?;
            writeln!(output)?;
            output.flush()?;
            Ok(())
        };
        if std::env::args().any(|arg| arg == "--batch") {
            previews.batch(serde_json::from_str(&input)?, &mut emit)?;
        } else {
            emit(previews.preview(input.trim())?)?;
        }
        Ok(())
    };
    if run().is_err() && !cancelled.load(Ordering::Relaxed) {
        println!("{{}}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Previews) {
        let root = tempfile::tempdir().unwrap();
        let previews =
            Previews::new(root.path().to_owned(), Arc::new(AtomicBool::new(false))).unwrap();
        (root, previews)
    }
    #[test]
    fn cache_contract_and_batch_order() -> Result<()> {
        let (_root, previews) = fixture();
        let url = "https://example.invalid/synthetic";
        let (key, receipt) = previews.paths(url);
        let image = previews.cache.join(format!("{key}.png"));
        fs::write(&image, b"synthetic image")?;
        let saved = json!({"previewVersion":4,"image":uri(&image)?,"imageKind":"page"});
        atomic_write(&receipt, &serde_json::to_vec(&saved)?)?;
        File::options()
            .write(true)
            .open(&receipt)?
            .set_modified(SystemTime::UNIX_EPOCH)?;
        assert_eq!(previews.cached(url), Some(saved.clone()));
        let mut rows = vec![];
        previews.batch(
            vec!["file:///uncached".into(), url.into(), url.into()],
            |row| {
                rows.push(row);
                Ok(())
            },
        )?;
        assert_eq!(
            rows,
            vec![
                json!({"url":url,"preview":saved}),
                json!({"url":"file:///uncached","preview":{}})
            ]
        );
        fs::remove_file(image)?;
        assert!(previews.cached(url).is_none());
        for content in ["{", "[]", "{\"previewVersion\":3}"] {
            fs::write(&receipt, content)?;
            assert!(previews.cached(url).is_none());
        }
        let saved = json!({"previewVersion":4,"unavailable":true});
        atomic_write(&receipt, &serde_json::to_vec(&saved)?)?;
        assert_eq!(previews.cached(url), Some(saved));
        File::options()
            .write(true)
            .open(&receipt)?
            .set_modified(SystemTime::now() - Duration::from_secs(301))?;
        assert!(previews.cached(url).is_none());
        Ok(())
    }
    #[test]
    fn cache_rejects_external_images() -> Result<()> {
        let (_root, previews) = fixture();
        let (_, receipt) = previews.paths("https://example.invalid");
        for image in [
            "https://example.invalid/a.png",
            "file:///etc/passwd",
            "file://remote/a.png",
        ] {
            atomic_write(
                &receipt,
                &serde_json::to_vec(&json!({"previewVersion":4,"image":image}))?,
            )?;
            assert!(previews.cached("https://example.invalid").is_none());
        }
        Ok(())
    }
    #[test]
    fn html_entities_metadata_and_failure_pages() {
        let meta = metadata("<title>A &amp; B</title><meta property='og:description' content='first &amp; best'><meta property='og:description' content='second'><link rel='shortcut ICON' href='/icon.png'>");
        assert_eq!(meta.title, "A & B");
        assert_eq!(meta.values["og:description"], "first & best");
        assert_eq!(meta.icons, vec!["/icon.png"]);
        assert_eq!(
            render_failure("<title>403 Access denied</title>"),
            "blocked"
        );
        assert_eq!(render_failure("<title>Sign in</title>"), "sign-in");
        assert_eq!(render_failure("<div id=\"main-frame-error\">"), "failed");
        assert_eq!(render_failure("<title>Ordinary page</title>"), "");
    }
}
