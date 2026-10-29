use super::process::*;
use crate::util::{atomic_json, now, private_dir};
use anyhow::{bail, ensure, Context, Result};
use clap::Parser;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    name = "synctjik cloudflare",
    about = "Back up Cloudflare DNS, KV and R2 resources"
)]
struct Cli {
    #[arg(default_value="backup", value_parser=["backup", "check"])]
    action: String,
    #[arg(long)]
    policy: Option<PathBuf>,
    /// Export sources, without accessing Restic. Still reads remote data.
    #[arg(long)]
    dry_run: bool,
}
#[derive(Deserialize)]
struct Policy {
    mount_uuid: String,
    mount_point: PathBuf,
    repository: PathBuf,
    account_id: String,
    restic_password: PathBuf,
    cloudflare_token: PathBuf,
    cloudflare_config_token: PathBuf,
    rclone_config: PathBuf,
    r2_remote: String,
    expected_r2_buckets: usize,
}
impl Policy {
    fn load(p: &Path) -> Result<Self> {
        let mut s: Self = serde_json::from_reader(crate::util::read_file(p)?)
            .context("Invalid Cloudflare policy")?;
        let h = home()?;
        for p in [
            &mut s.mount_point,
            &mut s.repository,
            &mut s.restic_password,
            &mut s.cloudflare_token,
            &mut s.cloudflare_config_token,
            &mut s.rclone_config,
        ] {
            *p = crate::util::expand(&path(p), &h);
            absolute(p)?;
        }
        ensure!(
            !s.mount_uuid.trim().is_empty()
                && !s.account_id.trim().is_empty()
                && !s.r2_remote.trim().is_empty()
                && s.expected_r2_buckets > 0,
            "Incomplete Cloudflare policy"
        );
        ensure!(
            s.repository != s.mount_point && s.repository.starts_with(&s.mount_point),
            "Repository must be inside approved mount"
        );
        id(&s.account_id)?;
        Ok(s)
    }
    fn verify_mount(&self, runner: &Runner, writable: bool) -> Result<()> {
        no_symlinks(&self.mount_point)?;
        ensure!(
            self.mount_point.is_dir() && mounted(&self.mount_point),
            "Approved backup drive is not mounted at its exact mount point"
        );
        let bytes = runner.run(
            Command::new("findmnt")
                .args(["--noheadings", "--raw", "--mountpoint"])
                .arg(&self.mount_point)
                .args(["--output", "UUID,OPTIONS"]),
        )?;
        let text = String::from_utf8(bytes)?;
        let fields: Vec<_> = text.split_whitespace().collect();
        ensure!(
            fields.first().copied() == Some(self.mount_uuid.as_str()),
            "Backup drive UUID does not match policy"
        );
        ensure!(
            !writable
                || fields
                    .get(1)
                    .is_some_and(|s| s.split(',').any(|s| s == "rw")),
            "Backup drive is read-only"
        );
        Ok(())
    }
    fn verify_repo(&self, runner: &Runner) -> Result<()> {
        self.verify_mount(runner, true)?;
        no_symlinks(&self.repository)?;
        let parent = self
            .repository
            .parent()
            .context("Missing repository parent")?;
        ensure!(
            !parent.exists() || parent.is_dir(),
            "Repository parent is not a directory"
        );
        Ok(())
    }
    fn restic(
        &self,
        runner: &Runner,
        args: &[&str],
        allow_failure: bool,
    ) -> Result<(i32, Vec<u8>)> {
        self.verify_repo(runner)?;
        runner.guarded(
            Command::new("restic")
                .args(args)
                .env("RESTIC_REPOSITORY", &self.repository)
                .env("RESTIC_PASSWORD_FILE", &self.restic_password)
                .env("RESTIC_PROGRESS_FPS", "0.1"),
            None,
            allow_failure,
        )
    }
    fn capacity(&self, bytes: u64) -> Result<()> {
        ensure!(
            free_space(&self.mount_point)?
                >= bytes.checked_add(1 << 30).context("R2 size overflow")?,
            "Backup drive lacks source capacity plus 1 GiB reserve"
        );
        Ok(())
    }
}
fn id(value: &str) -> Result<&str> {
    ensure!(
        !value.is_empty()
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "Invalid Cloudflare resource identifier"
    );
    Ok(value)
}
struct Api {
    client: reqwest::blocking::Client,
    base: String,
}
impl Api {
    fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(120))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .context("Unable to initialize HTTPS")?,
            base: "https://api.cloudflare.com/client/v4".into(),
        })
    }
    fn bytes(&self, token: &str, route: &str, query: &[(String, String)]) -> Result<Vec<u8>> {
        let response = self
            .client
            .get(format!("{}{route}", self.base))
            .bearer_auth(token)
            .query(query)
            .send()
            .map_err(|_| anyhow::anyhow!("Cloudflare request failed"))?;
        ensure!(
            response.status().is_success(),
            "Cloudflare request failed (HTTP {})",
            response.status().as_u16()
        );
        response
            .bytes()
            .map(|b| b.to_vec())
            .map_err(|_| anyhow::anyhow!("Unable to read Cloudflare response"))
    }
    fn get(
        &self,
        token: &str,
        route: &str,
        query: &[(String, String)],
    ) -> Result<(Value, Option<String>)> {
        let v: Value = serde_json::from_slice(&self.bytes(token, route, query)?)
            .context("Malformed Cloudflare JSON")?;
        ensure!(
            v["success"] == true,
            "Cloudflare returned an unsuccessful response"
        );
        Ok((
            v["result"].clone(),
            v["result_info"]["cursor"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        ))
    }
    fn pages(&self, token: &str, route: &str, extra: &[(String, String)]) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for page in 1..=100_000 {
            let mut query = extra.to_vec();
            query.extend([
                ("per_page".into(), "100".into()),
                ("page".into(), page.to_string()),
            ]);
            let (v, _) = self.get(token, route, &query)?;
            let values = v.as_array().context("Unexpected Cloudflare list")?;
            if !values.is_empty() {
                ensure!(
                    seen.insert(crate::util::digest(&v)),
                    "Cloudflare pagination repeated a page"
                );
            }
            out.extend(values.iter().cloned());
            if values.len() < 100 {
                return Ok(out);
            }
        }
        bail!("Cloudflare pagination exceeded safety bound")
    }
}
fn encoded(s: &str) -> String {
    // Encode even slashes in KV names as one URL segment.
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
#[derive(Default)]
struct Counts {
    zones: usize,
    records: usize,
    namespaces: usize,
    keys: usize,
    buckets: usize,
    objects: u64,
    bytes: u64,
}
fn export(
    api: &Api,
    p: &Policy,
    runner: &Runner,
    root: &Path,
    token: &str,
    dns_token: &str,
) -> Result<Counts> {
    for name in ["dns", "kv", "r2"] {
        let d = root.join(name);
        if d.exists() {
            fs::remove_dir_all(d)?;
        }
    }
    let mut counts = Counts::default();
    let zones = api.pages(
        token,
        "/zones",
        &[("account.id".into(), p.account_id.clone())],
    )?;
    ensure!(!zones.is_empty(), "Refusing empty DNS export");
    ensure!(
        zones.iter().all(|z| z["account"]["id"] == p.account_id),
        "Zone belongs to another account"
    );
    counts.zones = zones.len();
    for z in &zones {
        runner.check()?;
        let zid = id(z["id"].as_str().context("Zone ID missing")?)?;
        let (settings, _) = api.get(token, &format!("/zones/{zid}/settings"), &[])?;
        let records = api.pages(dns_token, &format!("/zones/{zid}/dns_records"), &[])?;
        counts.records += records.len();
        let d = root.join("dns").join(zid);
        atomic_json(&d.join("zone.json"), z)?;
        atomic_json(&d.join("settings.json"), &settings)?;
        atomic_json(&d.join("records.json"), &json!(records))?;
    }
    atomic_json(
        &root.join("dns/inventory.json"),
        &json!({"exported_at":now(),"zone_count":counts.zones,"dns_record_count":counts.records,"zones":zones.iter().map(|z| json!({"id":z["id"],"name":z["name"],"status":z["status"]})).collect::<Vec<_>>()}),
    )?;
    let base = format!("/accounts/{}/storage/kv/namespaces", p.account_id);
    let namespaces = api.pages(token, &base, &[])?;
    ensure!(!namespaces.is_empty(), "Refusing empty KV namespace export");
    counts.namespaces = namespaces.len();
    let mut value_bytes = 0u64;
    for ns in &namespaces {
        let nid = id(ns["id"].as_str().context("Namespace ID missing")?)?;
        let mut cursor = None;
        let mut seen = HashSet::new();
        let mut keys = Vec::new();
        loop {
            runner.check()?;
            let mut query = vec![("limit".into(), "1000".into())];
            if let Some(c) = cursor {
                query.push(("cursor".into(), c));
            }
            let (v, next) = api.get(token, &format!("{base}/{nid}/keys"), &query)?;
            keys.extend(
                v.as_array()
                    .context("Unexpected KV key list")?
                    .iter()
                    .cloned(),
            );
            match next {
                None => break,
                Some(c) => {
                    ensure!(seen.insert(c.clone()), "KV cursor repeated");
                    cursor = Some(c);
                }
            }
        }
        let dir = root.join("kv").join(nid);
        private_dir(&dir)?;
        let mut values = Vec::new();
        for key in keys {
            runner.check()?;
            let name = key["name"].as_str().context("KV key name missing")?;
            let bytes = api.bytes(
                token,
                &format!("{base}/{nid}/values/{}", encoded(name)),
                &[],
            )?;
            let digest = format!("{:x}", Sha256::digest(&bytes));
            let filename = format!("{digest}.value");
            fs::write(dir.join(&filename), &bytes)?;
            values.push(json!({"key":name,"metadata":key["metadata"],"expiration":key["expiration"],"expiration_ttl":key["expiration_ttl"],"file":filename,"size":bytes.len(),"sha256":digest}));
            value_bytes += bytes.len() as u64;
        }
        counts.keys += values.len();
        atomic_json(
            &dir.join("manifest.json"),
            &json!({"namespace":ns,"key_count":values.len(),"values":values}),
        )?;
    }
    atomic_json(
        &root.join("kv/inventory.json"),
        &json!({"exported_at":now(),"namespace_count":counts.namespaces,"key_count":counts.keys,"value_bytes":value_bytes,"namespaces":namespaces.iter().map(|n| json!({"id":n["id"],"title":n["title"]})).collect::<Vec<_>>()}),
    )?;
    let summary: Value = serde_json::from_slice(
        &runner.run(
            Command::new("rclone")
                .args(["size", &p.r2_remote, "--json", "--config"])
                .arg(&p.rclone_config),
        )?,
    )?;
    let listing = runner.run(
        Command::new("rclone")
            .args(["lsd", &p.r2_remote, "--config"])
            .arg(&p.rclone_config),
    )?;
    let text = String::from_utf8(listing)?;
    let buckets: Vec<_> = text
        .lines()
        .filter_map(|s| s.split_whitespace().last())
        .collect();
    ensure!(
        buckets.len() >= p.expected_r2_buckets,
        "R2 bucket inventory is incomplete"
    );
    counts.buckets = buckets.len();
    counts.objects = summary["count"]
        .as_u64()
        .context("Missing R2 object count")?;
    counts.bytes = summary["bytes"].as_u64().context("Missing R2 size")?;
    atomic_json(
        &root.join("r2/inventory.json"),
        &json!({"exported_at":now(),"bucket_count":counts.buckets,"object_count":counts.objects,"bytes":counts.bytes,"buckets":buckets,"metadata_note":"Object bodies, names and server modification timestamps are read through the read-only rclone mount. Custom S3 metadata is not guaranteed to be restored."}),
    )?;
    println!("Exports ready: {} zones, {} DNS records, {} KV namespaces, {} keys, {} R2 buckets, {} objects", counts.zones, counts.records, counts.namespaces, counts.keys, counts.buckets, counts.objects);
    Ok(counts)
}
struct Mount {
    child: ManagedChild,
    path: PathBuf,
}
impl Mount {
    fn start(p: &Policy, runner: &Runner, path: &Path, log: &Path) -> Result<Self> {
        private_dir(path)?;
        ensure!(!mounted(path), "Temporary R2 path already mounted");
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)?;
        let child = Command::new("rclone")
            .args(["mount", &p.r2_remote])
            .arg(path)
            .arg("--config")
            .arg(&p.rclone_config)
            .args([
                "--read-only",
                "--vfs-cache-mode",
                "off",
                "--dir-cache-time",
                "24h",
                "--vfs-refresh",
                "--fast-list",
                "--use-server-modtime",
                "--poll-interval",
                "0",
                "--log-level",
                "ERROR",
            ])
            .stdout(Stdio::null())
            .stderr(log)
            .process_group(0)
            .spawn()?;
        let mut s = Self {
            child: ManagedChild(child, true),
            path: path.into(),
        };
        let until = Instant::now() + Duration::from_secs(60);
        loop {
            runner.check()?;
            ensure!(s.child.0.try_wait()?.is_none(), "R2 mount process exited");
            if mounted(path) {
                return Ok(s);
            }
            ensure!(Instant::now() < until, "Timed out waiting for R2 mount");
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}
use std::os::unix::process::CommandExt;
impl Drop for Mount {
    fn drop(&mut self) {
        if mounted(&self.path) {
            for program in ["fusermount3", "fusermount"] {
                if available(program) {
                    let _ = Command::new(program)
                        .arg("-u")
                        .arg(&self.path)
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                    break;
                }
            }
        }
    }
}
// Never recursively remove a tree containing a mount, even when unmount failed.
struct Work(PathBuf);
impl Drop for Work {
    fn drop(&mut self) {
        if !mounted(&self.0.join("r2-mount")) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
fn backup(p: &Policy, runner: &Runner, state: &Path, dry: bool) -> Result<()> {
    backup_with_api(p, runner, state, dry, &Api::new()?)
}
fn backup_with_api(p: &Policy, runner: &Runner, state: &Path, dry: bool, api: &Api) -> Result<()> {
    if dry {
        p.verify_mount(runner, false)?;
    } else {
        p.verify_repo(runner)?;
        secret(&p.restic_password)?;
    }
    secret(&p.rclone_config)?;
    let dns = secret(&p.cloudflare_token)?;
    let token = secret(&p.cloudflare_config_token)?;
    private_dir(state)?;
    let _lock = Lock::take(&state.join("backup.lock"), false, runner)?;
    let work = Work(
        tempfile::Builder::new()
            .prefix("cloudflare-restic-")
            .tempdir()?
            .keep(),
    );
    let mut c = export(api, p, runner, &work.0, &token, &dns)?;
    if dry {
        println!("Dry run complete; no Restic operations performed");
        return Ok(());
    }
    p.verify_repo(runner)?;
    p.capacity(c.bytes)?;
    if !p.repository.join("config").exists() {
        ensure!(
            !p.repository.exists() || fs::read_dir(&p.repository)?.next().is_none(),
            "Refusing nonempty uninitialized repository"
        );
        private_dir(&p.repository)?;
        p.restic(runner, &["init"], false)?;
    }
    for attempt in 1..=2 {
        if attempt > 1 {
            c = export(api, p, runner, &work.0, &token, &dns)?;
            p.verify_repo(runner)?;
            p.capacity(c.bytes)?;
        }
        let mount_path = work.0.join("r2-mount");
        let mount = Mount::start(p, runner, &mount_path, &state.join("rclone-mount.log"))?;
        ensure!(
            fs::read_dir(&mount_path)?.next().is_some(),
            "Refusing empty R2 mount"
        );
        let (code, _) = p.restic(
            runner,
            &[
                "backup",
                "--json",
                "--ignore-inode",
                "--read-concurrency",
                "32",
                "--tag",
                "cloudflare",
                "--host",
                "cloudflare",
                &path(&work.0),
            ],
            true,
        )?;
        drop(mount);
        ensure!(
            !mounted(&mount_path),
            "R2 unmount failed; private work tree retained"
        );
        if code == 0 {
            break;
        }
        ensure!(
            code == 3 && attempt == 1,
            "Restic backup failed (exit {code}); partial snapshots retained"
        );
        println!("Incomplete source read; retrying once with fresh exports and mount");
    }
    p.restic(runner, &["check"], false)?;
    let (_, bytes) = p.restic(runner, &["snapshots", "--json"], false)?;
    let snapshots: Vec<Value> = serde_json::from_slice(&bytes)?;
    let latest = snapshots
        .iter()
        .filter(|s| {
            s["hostname"] == "cloudflare"
                && s["tags"]
                    .as_array()
                    .is_some_and(|t| t.contains(&json!("cloudflare")))
        })
        .max_by_key(|s| s["time"].as_str().unwrap_or(""))
        .context("Expected Cloudflare snapshot missing")?;
    ensure!(
        latest["id"].is_string()
            && latest["time"]
                .as_str()
                .is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok()),
        "Invalid snapshot metadata"
    );
    let marker = state.join("retention-enabled.json");
    if marker.exists() {
        p.restic(
            runner,
            &[
                "forget",
                "--host",
                "cloudflare",
                "--tag",
                "cloudflare",
                "--group-by",
                "host,tags",
                "--keep-weekly",
                "1",
                "--keep-monthly",
                "1",
                "--prune",
            ],
            false,
        )?;
    } else {
        atomic_json(
            &marker,
            &json!({"enabled_after":now(),"group_by":"host,tags","keep_weekly":1,"keep_monthly":1}),
        )?;
    }
    atomic_json(
        &state.join("last-success.json"),
        &json!({"completed":now(),"snapshot_id":latest["id"],"zones":c.zones,"dns_records":c.records,"kv_namespaces":c.namespaces,"kv_keys":c.keys,"r2_buckets":c.buckets,"r2_objects":c.objects,"r2_bytes":c.bytes}),
    )?;
    if state.join("last-failure.json").exists() {
        fs::remove_file(state.join("last-failure.json"))?;
    }
    println!("Cloudflare resources backup verified");
    Ok(())
}
pub fn main(args: &[String], runner: &Runner) -> Result<()> {
    let cli = match Cli::try_parse_from(
        std::iter::once("synctjik cloudflare".into()).chain(args.iter().cloned()),
    ) {
        Ok(c) => c,
        Err(e) => {
            let code = e.exit_code();
            e.print()?;
            ensure!(code == 0, "Invalid Cloudflare arguments");
            return Ok(());
        }
    };
    let h = home()?;
    let state = h.join(".local/state/cloudflare-restic");
    no_symlinks(&state)?;
    let result = (|| {
        let p = Policy::load(
            &cli.policy
                .unwrap_or_else(|| h.join(".config/synctjik/cloudflare.json")),
        )?;
        for c in ["rclone", "restic"] {
            ensure!(available(c), "Required dependency unavailable: {c}");
        }
        if cli.action == "check" {
            println!("Cloudflare policy and dependencies valid; no remote data accessed");
            Ok(())
        } else {
            backup(&p, runner, &state, cli.dry_run)
        }
    })();
    if let Err(ref e) = result {
        if cli.action != "check" {
            atomic_json(
                &state.join("last-failure.json"),
                &json!({"failed":now(),"error":e.to_string()}),
            )?;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    fn api_fixture(responses: Vec<(u16, Vec<u8>)>) -> (Api, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let mut paths = Vec::new();
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                paths.push(line);
                loop {
                    let mut l = String::new();
                    reader.read_line(&mut l).unwrap();
                    if l == "\r\n" || l.is_empty() {
                        break;
                    }
                }
                write!(
                    stream,
                    "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
            paths
        });
        (
            Api {
                client: reqwest::blocking::Client::builder()
                    .no_proxy()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .unwrap(),
                base,
            },
            worker,
        )
    }
    #[test]
    fn api_pages_preserve_queries_and_binary_values_and_sanitize_errors() {
        let first: Vec<_> = (0..100).map(|i| json!({"id":i})).collect();
        let (api, worker) = api_fixture(vec![
            (
                200,
                serde_json::to_vec(&json!({"success":true,"result":first})).unwrap(),
            ),
            (200, br#"{"success":true,"result":[{"id":100}]}"#.to_vec()),
            (200, vec![0, 255, 1]),
            (403, b"secret-provider-body".to_vec()),
        ]);
        assert_eq!(
            api.pages("fixture", "/zones", &[("account.id".into(), "test".into())])
                .unwrap()
                .len(),
            101
        );
        assert_eq!(
            api.bytes("fixture", "/values/a%2Fb", &[]).unwrap(),
            vec![0, 255, 1]
        );
        let error = api
            .bytes("fixture", "/values/private-name", &[])
            .unwrap_err()
            .to_string();
        assert!(error.contains("403"));
        assert!(!error.contains("secret"));
        assert!(!error.contains("private-name"));
        let requests = worker.join().unwrap();
        assert!(requests[0].contains("page=1"));
        assert!(requests[1].contains("page=2"));
        assert!(requests[0].contains("account.id=test"));
        assert_eq!(encoded("a/b ?é"), "a%2Fb%20%3F%C3%A9");
        assert!(id("../escape").is_err());
    }
    #[test]
    fn mount_guard_refuses_plain_directories_and_credentials_refuse_symlinks() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let token = root.join("token");
        fs::write(&token, "fixture").unwrap();
        fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(secret(&token).unwrap(), "fixture");
        symlink(&token, root.join("link")).unwrap();
        assert!(secret(&root.join("link")).is_err());
        fs::set_permissions(&token, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(secret(&token).is_err());
        assert!(!mounted(root));
        assert!(absolute(Path::new("/tmp/../escape")).is_err());
        symlink(root, root.join("redirect")).unwrap();
        assert!(no_symlinks(&root.join("redirect/missing/repo")).is_err());
    }
    #[test]
    fn mock_backup_flow_retries_only_partial_reads_and_gates_retention() {
        for scenario in ["retry", "fatal", "success"] {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "backends::cloudflare::tests::mock_backup_worker",
                    "--ignored",
                    "--nocapture",
                ])
                .env("SYNCTJIK_FIXTURE_SCENARIO", scenario)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }
    #[test]
    #[ignore = "isolated child fixture, launched by mock_backup_flow"]
    fn mock_backup_worker() {
        use std::os::unix::fs::PermissionsExt;
        let scenario = std::env::var("SYNCTJIK_FIXTURE_SCENARIO").expect("fixture child only");
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let bin = root.join("bin");
        fs::create_dir(&bin).unwrap();
        let script = r#"#!/bin/sh
set -eu
name=${0##*/}
case "$name" in
mountpoint) [ "$2" = "$FIXTURE/disk" ] || [ -f "$2/.mounted" ];;
findmnt) echo 'fixture rw,relatime';;
fusermount3) rm -f "$2/.mounted";;
rclone)
    case "$1" in
    size) echo '{"count":1,"bytes":1}';;
    lsd) echo '0 2026-01-01 00:00:00 -1 fixture';;
    mount) touch "$3/.mounted"; exec sleep 300;;
    esac;;
restic)
    echo "$1" >> "$FIXTURE/calls"
    case "$1" in
    init) touch "$RESTIC_REPOSITORY/config";;
    backup)
        if [ "$SYNCTJIK_FIXTURE_SCENARIO" = fatal ]; then exit 7; fi
        if [ "$SYNCTJIK_FIXTURE_SCENARIO" = retry ] && [ ! -e "$FIXTURE/retried" ]; then touch "$FIXTURE/retried"; exit 3; fi;;
    snapshots) echo '[{"id":"aaaaaaaa","time":"2026-10-29T12:00:00Z","hostname":"cloudflare","tags":["cloudflare"]}]';;
    esac;;
esac
"#;
        for name in ["rclone", "restic", "findmnt", "mountpoint", "fusermount3"] {
            let file = bin.join(name);
            fs::write(&file, script).unwrap();
            fs::set_permissions(file, fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::env::set_var("PATH", format!("{}:/usr/bin:/bin", bin.display()));
        std::env::set_var("FIXTURE", root);
        let disk = root.join("disk");
        fs::create_dir(&disk).unwrap();
        let credential = root.join("credential");
        fs::write(&credential, "fixture").unwrap();
        fs::set_permissions(&credential, fs::Permissions::from_mode(0o600)).unwrap();
        let p = Policy {
            mount_uuid: "fixture".into(),
            mount_point: disk.clone(),
            repository: disk.join("restic"),
            account_id: "account".into(),
            restic_password: credential.clone(),
            cloudflare_token: credential.clone(),
            cloudflare_config_token: credential.clone(),
            rclone_config: credential,
            r2_remote: "fixture:".into(),
            expected_r2_buckets: 1,
        };
        let export = vec![
            json!({"success":true,"result":[{"id":"zone","account":{"id":"account"}}]}),
            json!({"success":true,"result":[]}),
            json!({"success":true,"result":[]}),
            json!({"success":true,"result":[{"id":"namespace"}]}),
            json!({"success":true,"result":[]}),
        ];
        let passes = if scenario == "retry" || scenario == "success" {
            2
        } else {
            1
        };
        let responses = (0..passes)
            .flat_map(|_| export.iter().map(|v| (200, serde_json::to_vec(v).unwrap())))
            .collect();
        let (api, server) = api_fixture(responses);
        let runner = Runner::new().unwrap();
        let state = root.join("state");
        let result = backup_with_api(&p, &runner, &state, false, &api);
        if scenario == "fatal" {
            assert!(result.is_err());
            assert!(!state.join("last-success.json").exists());
            assert!(!state.join("retention-enabled.json").exists());
        } else {
            result.unwrap();
            assert!(state.join("last-success.json").exists());
            assert!(!fs::read_to_string(root.join("calls"))
                .unwrap()
                .contains("forget"));
        }
        if scenario == "success" {
            backup_with_api(&p, &runner, &state, false, &api).unwrap();
            assert!(fs::read_to_string(root.join("calls"))
                .unwrap()
                .contains("forget"));
        }
        let calls = fs::read_to_string(root.join("calls")).unwrap();
        assert_eq!(calls.lines().filter(|l| *l == "backup").count(), passes);
        server.join().unwrap();
    }
}
