use super::*;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    time::Duration,
};
pub fn client(seconds: u64) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(seconds))
        .build()?)
}
fn response_json(response: reqwest::blocking::Response) -> Result<Value> {
    if !response.status().is_success() {
        bail!(
            "application API request failed (HTTP {})",
            response.status().as_u16()
        );
    }
    let mut body = Vec::new();
    response.take(16 * 1024 * 1024 + 1).read_to_end(&mut body)?;
    if body.len() > 16 * 1024 * 1024 {
        bail!("application API response too large");
    }
    if body.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&body).context("invalid application API JSON")
}
pub fn xml_text(path: &Path, element: &str) -> Result<String> {
    let text = fs::read_to_string(path)?;
    let mut reader = quick_xml::Reader::from_str(&text);
    let mut inside = false;
    loop {
        match reader.read_event()? {
            quick_xml::events::Event::Start(e) => inside = e.name().as_ref() == element.as_bytes(),
            quick_xml::events::Event::Text(e) if inside => {
                return Ok(quick_xml::escape::unescape(&e.decode()?)?.into_owned())
            }
            quick_xml::events::Event::Eof => bail!("application API key missing"),
            _ => (),
        }
    }
}
struct Transmission {
    client: reqwest::blocking::Client,
    auth: String,
    session: String,
    url: String,
}
impl Transmission {
    fn new(policy: &Value) -> Result<Self> {
        let data: Value = serde_json::from_str(&command(&[
            "docker",
            "inspect",
            text(&policy["media"], "container")?,
        ])?)?;
        let env: BTreeMap<_, _> = data[0]["Config"]["Env"]
            .as_array()
            .context("container credentials unavailable")?
            .iter()
            .filter_map(|v| v.as_str().and_then(|s| s.split_once('=')))
            .collect();
        let user = env.get("USER").context("Transmission user unavailable")?;
        let pass = env
            .get("PASS")
            .context("Transmission password unavailable")?;
        use base64::Engine;
        let auth = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
        let url = text(&policy["media"], "rpc")?.to_owned();
        loopback(&url)?;
        Ok(Self {
            client: client(120)?,
            auth,
            session: String::new(),
            url,
        })
    }
    fn rpc(&mut self, method: &str, args: Value) -> Result<Value> {
        for _ in 0..2 {
            let response = self
                .client
                .post(&self.url)
                .header("Authorization", format!("Basic {}", self.auth))
                .header("X-Transmission-Session-Id", &self.session)
                .json(&json!({"method":method,"arguments":args}))
                .send()
                .context("Transmission connection failed")?;
            if response.status().as_u16() == 409 {
                self.session = response
                    .headers()
                    .get("X-Transmission-Session-Id")
                    .context("Transmission session header missing")?
                    .to_str()?
                    .into();
                continue;
            }
            let data = response_json(response)?;
            if data["result"] != "success" {
                bail!("Transmission RPC failed");
            }
            return Ok(data["arguments"].clone());
        }
        bail!("Transmission session negotiation failed")
    }
    fn torrents(&mut self) -> Result<Vec<Value>> {
        let value=self.rpc("torrent-get",json!({"fields":["id","hashString","name","labels","metadataPercentComplete","percentDone","leftUntilDone","totalSize","files","downloadDir","secondsSeeding","isPrivate","trackers"]}))?;
        Ok(value["torrents"]
            .as_array()
            .context("torrent inventory missing")?
            .clone())
    }
}
fn loopback(url: &str) -> Result<()> {
    let url = reqwest::Url::parse(url)?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("application API must use direct loopback HTTP");
    }
    Ok(())
}
fn tracker_kind(torrent: &Value, policy: &Value) -> Result<&'static str> {
    let Some(trackers) = torrent["trackers"].as_array().filter(|a| !a.is_empty()) else {
        return Ok("unknown");
    };
    let known = policy["media"]["private_tracker_hosts"]
        .as_array()
        .context("private tracker policy missing")?;
    let domains = policy["media"]["private_tracker_domains"]
        .as_array()
        .context("private tracker domains missing")?;
    let mut private = false;
    for tracker in trackers {
        let Some(url) = tracker["announce"]
            .as_str()
            .and_then(|s| reqwest::Url::parse(s).ok())
        else {
            return Ok("unknown");
        };
        if !["http", "https", "udp"].contains(&url.scheme()) {
            return Ok("unknown");
        }
        let Some(host) = url.host_str() else {
            return Ok("unknown");
        };
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        private |= known.iter().any(|v| v == &host)
            || domains
                .iter()
                .filter_map(Value::as_str)
                .any(|d| host == d || host.ends_with(&format!(".{d}")));
    }
    Ok(if private {
        "ipt"
    } else if torrent["isPrivate"] == false {
        "public"
    } else {
        "unknown"
    })
}
fn decision(torrent: &Value, policy: &Value) -> Result<(bool, &'static str)> {
    let labels = policy["media"]["labels"]
        .as_array()
        .context("torrent labels missing")?;
    if !torrent["labels"]
        .as_array()
        .is_some_and(|a| a.iter().any(|v| labels.contains(v)))
    {
        return Ok((false, "unmanaged-label"));
    }
    if torrent["metadataPercentComplete"].as_f64() != Some(1.)
        || torrent["percentDone"].as_f64() != Some(1.)
        || torrent["leftUntilDone"].as_u64() != Some(0)
        || torrent["totalSize"].as_u64().unwrap_or(0) == 0
        || torrent["files"].as_array().is_none_or(|a| {
            a.is_empty()
                || a.iter().any(|f| {
                    f["bytesCompleted"].as_u64().is_none() || f["bytesCompleted"] != f["length"]
                })
        })
    {
        return Ok((false, "incomplete"));
    }
    let kind = tracker_kind(torrent, policy)?;
    if kind == "unknown" {
        return Ok((false, "unknown-tracker"));
    }
    if kind == "ipt"
        && torrent["secondsSeeding"].as_f64().is_none_or(|s| {
            s <= policy["media"]["minimum_seed_seconds"]
                .as_f64()
                .unwrap_or(f64::INFINITY)
        })
    {
        return Ok((false, "ipt-under-two-weeks"));
    }
    Ok((true, kind))
}
fn resolved(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        bail!("absolute storage path required");
    }
    let mut current = PathBuf::from("/");
    for part in path.components() {
        match part {
            std::path::Component::RootDir => (),
            std::path::Component::Normal(name) => {
                current.push(name);
                if fs::symlink_metadata(&current).is_ok() {
                    current = current.canonicalize()?;
                }
            }
            _ => bail!("unsafe storage path"),
        }
    }
    Ok(current)
}
fn paths(torrent: &Value, policy: &Value) -> Result<Vec<PathBuf>> {
    let root = Path::new(text(&policy["media"], "downloads")?);
    let directory = Path::new(text(torrent, "downloadDir")?);
    if directory
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("unsafe download directory");
    }
    let mapped = policy["media"]["download_prefixes"]
        .as_array()
        .context("download prefixes missing")?
        .iter()
        .filter_map(Value::as_str)
        .find_map(|p| directory.strip_prefix(p).ok())
        .context("download directory outside managed storage")?;
    let directory = root.join(mapped);
    let root = resolved(root)?;
    let mut output = Vec::new();
    for file in torrent["files"]
        .as_array()
        .context("torrent files missing")?
    {
        let relative = Path::new(text(file, "name")?);
        if relative.as_os_str().is_empty()
            || relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            bail!("unsafe torrent file path");
        }
        let path = directory.join(relative);
        if !resolved(&path)?.starts_with(&root) {
            bail!("torrent file escapes download storage");
        }
        output.push(path);
    }
    Ok(output)
}
fn removal_receipt(torrent: &Value, reason: &str, action: &str) -> Value {
    json!({"time":chrono::Utc::now().to_rfc3339(),"hash":torrent["hashString"],"name":torrent["name"],"labels":torrent["labels"],"bytes":torrent["totalSize"],"seconds_seeding":torrent["secondsSeeding"],"reason":reason,"action":action})
}
fn audit(path: &Path, value: &Value) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
pub fn cleanup(policy: &Value, apply: bool) -> Result<i32> {
    let state = Path::new(text(&policy["media"], "cleanup_state")?);
    private_dir(state)?;
    let _lock = crate::process::Lock::acquire(&state.join("lock"))?;
    let mut client = Transmission::new(policy)?;
    let initial = client.torrents()?;
    let mut counts = BTreeMap::<String, u64>::new();
    for candidate in &initial {
        let (eligible, reason) = decision(candidate, policy)?;
        if !eligible {
            *counts.entry(reason.into()).or_default() += 1;
            continue;
        }
        let planned = paths(candidate, policy)?;
        if !apply {
            println!("{}", removal_receipt(candidate, reason, "would-remove"));
            *counts.entry("would-remove".into()).or_default() += 1;
            continue;
        }
        let current = client.torrents()?;
        let Some(torrent) = current
            .iter()
            .find(|t| t["hashString"] == candidate["hashString"])
        else {
            *counts.entry("already-removed".into()).or_default() += 1;
            continue;
        };
        let (eligible, reason) = decision(torrent, policy)?;
        if !eligible || paths(torrent, policy)? != planned {
            *counts.entry("changed-since-plan".into()).or_default() += 1;
            continue;
        }
        let mut root_torrent = torrent.clone();
        root_torrent["files"]=json!(torrent["files"].as_array().unwrap().iter().map(|f|json!({"name":Path::new(f["name"].as_str().unwrap()).components().next().unwrap().as_os_str()})).collect::<Vec<_>>());
        let roots: Vec<_> = paths(&root_torrent, policy)?
            .iter()
            .map(|p| resolved(p))
            .collect::<Result<_>>()?;
        let mut shared = false;
        for other in &current {
            if other["hashString"] != torrent["hashString"] {
                for path in paths(other, policy)? {
                    let path = resolved(&path)?;
                    shared |= roots.iter().any(|root| path.starts_with(root));
                }
            }
        }
        if shared {
            *counts.entry("shared-files".into()).or_default() += 1;
            continue;
        }
        let mut record = removal_receipt(torrent, reason, "remove-requested");
        audit(&state.join("removals.jsonl"), &record)?;
        client.rpc(
            "torrent-remove",
            json!({"ids":[torrent["hashString"]],"delete-local-data":true}),
        )?;
        let mut verified = false;
        for _ in 0..30 {
            let remaining = client.rpc(
                "torrent-get",
                json!({"ids":[torrent["hashString"]],"fields":["hashString"]}),
            )?;
            if remaining["torrents"]
                .as_array()
                .is_some_and(|a| a.is_empty())
                && !planned.iter().any(|p| fs::symlink_metadata(p).is_ok())
            {
                verified = true;
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        if !verified {
            bail!("torrent removal verification failed");
        }
        record["action"] = json!("removed");
        audit(&state.join("removals.jsonl"), &record)?;
        println!("{record}");
        *counts.entry("removed".into()).or_default() += 1;
    }
    println!(
        "{}",
        json!({"apply":apply,"scanned":initial.len(),"counts":counts})
    );
    Ok(0)
}
struct Catalog {
    client: reqwest::blocking::Client,
    base: String,
    key: String,
}
impl Catalog {
    fn request(&self, path: &str, delete: bool) -> Result<Value> {
        let url = format!("{}/{}", self.base.trim_end_matches('/'), path);
        let builder = if delete {
            self.client.delete(url)
        } else {
            self.client.get(url)
        };
        response_json(
            builder
                .header("X-Api-Key", &self.key)
                .send()
                .context("catalog connection failed")?,
        )
    }
}
pub fn guard(policy: &Value, dry: bool) -> Result<i32> {
    let p = &policy["media"];
    let state = Path::new(text(p, "guard_state")?);
    private_dir(state)?;
    let _lock = crate::process::Lock::acquire(&state.join("lock"))?;
    let mut client = Transmission::new(policy)?;
    let mut catalogs = BTreeMap::new();
    let mut claims = BTreeMap::<String, (String, Value)>::new();
    let mut errors = Vec::new();
    for (name, spec) in p["catalogs"]
        .as_object()
        .context("catalog policy missing")?
    {
        let attempt = (|| -> Result<()> {
            let base = text(spec, "url")?.to_owned();
            loopback(&base)?;
            let api = Catalog {
                client: self::client(30)?,
                base,
                key: xml_text(Path::new(text(spec, "config")?), "ApiKey")?,
            };
            let (mut page, mut count) = (1, 0);
            loop {
                let response = api.request(&format!("queue?pageSize=1000&page={page}"), false)?;
                let records = response["records"]
                    .as_array()
                    .context("queue records missing")?;
                count += records.len();
                for row in records {
                    if let Some(hash) = row["downloadId"].as_str().filter(|s| !s.is_empty()) {
                        let hash = hash.to_ascii_lowercase();
                        if claims.get(&hash).is_some_and(|(app, _)| app != name) {
                            bail!("torrent claimed by multiple applications");
                        }
                        claims.insert(hash, (name.clone(), row.clone()));
                    }
                }
                if records.is_empty()
                    || count as u64
                        >= response["totalRecords"]
                            .as_u64()
                            .context("queue total missing")?
                {
                    break;
                }
                page += 1;
                if page > 10000 {
                    bail!("queue pagination did not converge");
                }
            }
            catalogs.insert(name.clone(), api);
            Ok(())
        })();
        if attempt.is_err() {
            errors.push(name.clone());
        }
    }
    if !errors.is_empty() {
        bail!("catalog inventory incomplete or ambiguous; no torrents changed");
    }
    let torrents = client.torrents()?;
    let mut checked = BTreeMap::<String, u64>::new();
    let mut bad = 0;
    for torrent in torrents {
        let hash = text(&torrent, "hashString")?.to_ascii_lowercase();
        if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("invalid torrent identity");
        }
        let Some((app, row)) = claims.get(&hash) else {
            continue;
        };
        let Some(api) = catalogs.get(app) else {
            continue;
        };
        if torrent["metadataPercentComplete"].as_f64().unwrap_or(0.) < 1. {
            continue;
        }
        *checked.entry(app.clone()).or_default() += 1;
        let receipt_path = state.join(format!("{hash}.json"));
        if receipt_path.exists() {
            if !dry {
                client.rpc("torrent-stop", json!({"ids":[torrent["id"]]}))?;
                client.rpc(
                    "torrent-remove",
                    json!({"ids":[torrent["id"]],"delete-local-data":false}),
                )?;
            }
            continue;
        }
        let mut reasons = BTreeSet::new();
        for file in torrent["files"]
            .as_array()
            .context("torrent files missing")?
        {
            let name = text(file, "name")?;
            let normalized = name
                .replace('\\', "/")
                .trim_end_matches([' ', '.'])
                .to_ascii_lowercase();
            let extension = Path::new(&normalized)
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| format!(".{s}"));
            if extension.as_ref().is_some_and(|ext| {
                p["unsafe_extensions"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == ext))
            }) {
                reasons.insert(name.to_owned());
            }
            if file["bytesCompleted"].as_u64().unwrap_or(0) < file["length"].as_u64().unwrap_or(1) {
                continue;
            }
            let mut one = torrent.clone();
            one["files"] = json!([file]);
            if let Ok(paths) = paths(&one, policy) {
                let path = &paths[0];
                if path.is_file() {
                    let mut head = [0; 2];
                    if fs::File::open(path)?.read(&mut head)? == 2 && head == *b"MZ" {
                        reasons.insert(format!("{name} [Windows executable header]"));
                    }
                }
            }
        }
        if reasons.is_empty() {
            continue;
        }
        bad += 1;
        if dry {
            println!(
                "{}",
                json!({"would_reject":torrent["name"],"reasons":reasons})
            );
            continue;
        }
        let mut recent = 0;
        for entry in fs::read_dir(state)? {
            let path = entry?.path();
            if path.extension().is_some_and(|s| s == "json") {
                if let Ok(record) = read(&path) {
                    if record["replacement_requested"] == true
                        && record["blocked_at"]
                            .as_str()
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .is_some_and(|t| {
                                (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds()
                                    < 3600
                            })
                    {
                        recent += 1;
                    }
                }
            }
        }
        let redownload = recent < p["replacement_limit_per_hour"].as_u64().unwrap_or(3);
        client.rpc("torrent-stop", json!({"ids":[torrent["id"]]}))?;
        let id = row["id"].as_u64().context("queue identity missing")?;
        api.request(
            &format!(
                "queue/{id}?removeFromClient=false&blocklist=true&skipRedownload={}",
                !redownload
            ),
            true,
        )?;
        receipt(
            &receipt_path,
            &json!({"hash":torrent["hashString"],"name":torrent["name"],"reasons":reasons,"blocked_at":chrono::Utc::now().to_rfc3339(),"replacement_requested":redownload,"data_preserved":true,"application":app}),
        )?;
        client.rpc(
            "torrent-remove",
            json!({"ids":[torrent["id"]],"delete-local-data":false}),
        )?;
        println!("Rejected unsafe {app} torrent");
    }
    println!(
        "{}",
        json!({"checked_torrents":checked,"unsafe":bad,"dry_run":dry})
    );
    if !errors.is_empty() {
        bail!("some catalogs could not be checked: {}", errors.join(", "));
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn torrent_removal_requires_complete_managed_payload_and_strict_private_seed_age() {
        let policy = json!({"media":{"labels":["sonarr","radarr"],"minimum_seed_seconds":1209600,"private_tracker_hosts":["tracker.example"],"private_tracker_domains":["private.example"]}});
        let torrent = json!({"labels":["sonarr"],"metadataPercentComplete":1.0,"percentDone":1.0,"leftUntilDone":0,"totalSize":10,"files":[{"length":10,"bytesCompleted":10}],"isPrivate":false,"trackers":[{"announce":"udp://public.example/announce"}],"secondsSeeding":0});
        assert_eq!(decision(&torrent, &policy).unwrap(), (true, "public"));
        for (key, value) in [
            ("labels", json!([])),
            ("metadataPercentComplete", json!(0.9)),
            ("percentDone", json!(0.9)),
            ("leftUntilDone", json!(1)),
            ("totalSize", json!(0)),
            ("files", json!([])),
            ("trackers", json!([])),
            ("isPrivate", json!(true)),
        ] {
            let mut t = torrent.clone();
            t[key] = value;
            assert!(!decision(&t, &policy).unwrap().0, "{key}");
        }
        let mut private = torrent;
        private["trackers"] = json!([{"announce":"https://a.private.example/announce"}]);
        private["isPrivate"] = json!(true);
        for seconds in [0, 1209599, 1209600] {
            private["secondsSeeding"] = json!(seconds);
            assert!(!decision(&private, &policy).unwrap().0);
        }
        private["secondsSeeding"] = json!(1209601);
        assert_eq!(decision(&private, &policy).unwrap(), (true, "ipt"));
        private["trackers"] = json!([{"announce":"http://private.example.evil/"}]);
        assert!(!decision(&private, &policy).unwrap().0);
    }
    #[test]
    fn application_api_rejects_remote_urls_and_credentials() {
        assert!(loopback("http://127.0.0.1:9091/transmission/rpc").is_ok());
        for url in [
            "http://localhost:9091",
            "http://127.0.0.1.evil",
            "http://user:pass@127.0.0.1",
            "https://127.0.0.1",
            "http://192.0.2.1",
        ] {
            assert!(loopback(url).is_err());
        }
    }
}
