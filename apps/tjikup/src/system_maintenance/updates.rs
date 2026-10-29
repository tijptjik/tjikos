use super::*;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
fn report(stage: &str, subject: &str, note: &str) {
    println!(
        "{}\t{}\t{}",
        stage.replace(['\n', '\t'], " "),
        subject.replace(['\n', '\t'], " "),
        note.replace(['\n', '\t'], " ")
    );
}
struct Updater<'a> {
    policy: &'a Value,
    record: PathBuf,
    deadline: Instant,
    sequence: AtomicUsize,
}
impl Updater<'_> {
    fn cmd(
        &self,
        args: &[String],
        seconds: u64,
        accepted: &[i32],
    ) -> Result<crate::command_io::Output> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("update time budget exhausted; retry next run");
        }
        let result = crate::command_io::capture(
            Command::new(&args[0]).args(&args[1..]),
            remaining.min(Duration::from_secs(seconds)),
        )
        .with_context(|| format!("{} failed or timed out", args[0]))?;
        if !accepted.contains(&result.code) {
            let n = self.sequence.fetch_add(1, Ordering::Relaxed);
            let log = self.record.join(format!("error-{n}.log"));
            atomic(
                &log,
                format!("{}{}", result.stdout, result.stderr).as_bytes(),
                0o600,
            )?;
            bail!("{} failed; private diagnostics: {}", args[0], log.display());
        }
        Ok(result)
    }
    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }
    fn command(&self, args: &[&str]) -> Result<String> {
        Ok(self.cmd(&Self::strings(args), 120, &[0])?.stdout)
    }
    fn compose(&self, extra: &[&str]) -> Result<Vec<String>> {
        let mut args = self.policy["updates"]["compose"]
            .as_array()
            .context("compose policy missing")?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .context("compose argument missing")
            })
            .collect::<Result<Vec<_>>>()?;
        args.extend(Self::strings(extra));
        Ok(args)
    }
    fn backup_ready(&self) -> Result<bool> {
        let p = &self.policy["updates"];
        let max = p["max_backup_age_seconds"]
            .as_i64()
            .context("backup age missing")?;
        if let Ok(receipt) = read(Path::new(text(p, "backup_receipt")?)) {
            if receipt["completedAt"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .is_some_and(|t| {
                    let age = chrono::Utc::now() - t.with_timezone(&chrono::Utc);
                    age.num_seconds() >= 0 && age.num_seconds() <= max
                })
            {
                return Ok(true);
            }
        }
        self.command(&["systemctl", "start", "--no-block", text(p, "backup_unit")?])?;
        report(
            "SKIP",
            "App updates",
            "waiting for a fresh backup; retry after backup finishes",
        );
        Ok(false)
    }
    fn sessions(&self) -> Result<u64> {
        let text = fs::read_to_string(text(&self.policy["updates"], "plex_preferences")?)?;
        let mut reader = quick_xml::Reader::from_str(&text);
        let mut token = None;
        loop {
            match reader.read_event()? {
                quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                    for attr in e.attributes() {
                        let attr = attr?;
                        if attr.key.as_ref() == b"PlexOnlineToken" {
                            token = Some(attr.unescape_value()?.into_owned());
                        }
                    }
                    if token.is_some() {
                        break;
                    }
                }
                quick_xml::events::Event::Eof => break,
                _ => (),
            }
        }
        let token = token
            .filter(|s| !s.is_empty())
            .context("Plex token unavailable")?;
        let response = media::client(8)?
            .get("http://127.0.0.1:32400/status/sessions")
            .header("X-Plex-Token", token)
            .header("Accept", "application/xml")
            .send()
            .context("Plex session check failed")?;
        if response.status().as_u16() != 200 {
            bail!("Plex session check failed");
        }
        let mut bytes = Vec::new();
        response.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 1024 * 1024 {
            bail!("Plex session response too large");
        }
        session_count(&String::from_utf8(bytes)?)
    }
    fn plex_idle(&self) -> bool {
        match self.sessions() {
            Ok(0) => true,
            Ok(n) => {
                report("SKIP", "Plex", &format!("{n} active session(s)"));
                false
            }
            Err(_) => {
                report(
                    "SKIP",
                    "Plex",
                    "session check unavailable; keeping current version",
                );
                false
            }
        }
    }
    fn plex(&self) -> Result<()> {
        let installed = self.cmd(
            &Self::strings(&["rpm", "-q", "plexmediaserver"]),
            120,
            &[0, 1],
        )?;
        if installed.code != 0 {
            report("SKIP", "Plex", "not installed");
            return Ok(());
        }
        let old = self.command(&["rpm", "-q", "--qf", "%{VERSION}", "plexmediaserver"])?;
        if !self.plex_idle() {
            return Ok(());
        }
        let available = self.cmd(
            &Self::strings(&["dnf", "-q", "check-upgrade", "plexmediaserver"]),
            120,
            &[0, 100],
        )?;
        if available.code == 0 {
            report("SKIP", "Plex", "no changes");
            return Ok(());
        }
        if !self.backup_ready()? {
            return Ok(());
        }
        report("SYNC", "Plex", "downloading update");
        self.cmd(
            &Self::strings(&["dnf", "-y", "upgrade", "--downloadonly", "plexmediaserver"]),
            300,
            &[0],
        )?;
        if !self.plex_idle() {
            return Ok(());
        }
        report("UPDATE", "Plex", "no sessions; installing");
        self.cmd(
            &Self::strings(&["dnf", "-y", "--cacheonly", "upgrade", "plexmediaserver"]),
            300,
            &[0],
        )?;
        self.command(&["systemctl", "is-active", "--quiet", "plexmediaserver"])?;
        let version = self.command(&["rpm", "-q", "--qf", "%{VERSION}", "plexmediaserver"])?;
        report("UPDATE", &format!("Plex ({})", old.trim()), version.trim());
        Ok(())
    }
    fn containers(&self) -> Result<BTreeMap<String, Vec<Value>>> {
        let output = self.cmd(
            &self.compose(&["ps", "--status", "running", "--quiet"])?,
            120,
            &[0],
        )?;
        let ids: Vec<_> = output.stdout.split_whitespace().collect();
        if ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        let mut args = vec!["docker", "inspect"];
        args.extend(ids);
        let rows: Vec<Value> = serde_json::from_str(&self.command(&args)?)?;
        let mut services = BTreeMap::<String, Vec<Value>>::new();
        for row in rows {
            let labels = &row["Config"]["Labels"];
            if labels["com.docker.compose.oneoff"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case("true"))
            {
                continue;
            }
            if let Some(service) = labels["com.docker.compose.service"].as_str() {
                services.entry(service.into()).or_default().push(
                    json!({"id":row["Id"],"image":row["Image"],"image_ref":row["Config"]["Image"]}),
                );
            }
        }
        Ok(services)
    }
    fn image_version(&self, id: &str) -> String {
        let fallback = id.trim_start_matches("sha256:").chars().take(12).collect();
        let Ok(raw) = self.command(&[
            "docker",
            "image",
            "inspect",
            "--format",
            "{{json .Config.Labels}}",
            id,
        ]) else {
            return fallback;
        };
        let Ok(labels) = serde_json::from_str::<Value>(&raw) else {
            return fallback;
        };
        let version = labels["org.opencontainers.image.version"]
            .as_str()
            .or_else(|| labels["org.label-schema.version"].as_str())
            .or_else(|| {
                labels["build_version"]
                    .as_str()
                    .and_then(|s| s.split_once("version:-"))
                    .and_then(|(_, s)| s.split_whitespace().next())
            });
        if let Some(version) = version {
            let version = version
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>()
                .trim()
                .to_owned();
            if !version.is_empty()
                && version.len() <= 40
                && !["latest", "main", "master", "develop", "unknown"]
                    .contains(&version.to_ascii_lowercase().as_str())
            {
                return version;
            }
        }
        fallback
    }
    fn prepare(
        &self,
        name: &str,
        spec: &Value,
        containers: &[Value],
        project: &str,
    ) -> Result<(Option<String>, String)> {
        if let Some(note) = self.policy["updates"]["frozen_services"][name].as_str() {
            return Ok((None, note.into()));
        }
        let image = spec["image"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or(format!("{project}-{name}"));
        if image.contains("@sha256:") {
            return Ok((None, "digest pinned".into()));
        }
        if spec.get("build").is_some() {
            self.cmd(&self.compose(&["build", "--pull", name])?, 300, &[0])?;
        } else {
            self.cmd(
                &self.compose(&["pull", "--policy", "always", name])?,
                120,
                &[0],
            )?;
        }
        let id = self
            .command(&["docker", "image", "inspect", "--format", "{{.Id}}", &image])?
            .trim()
            .to_owned();
        if id.is_empty() {
            bail!("image identity unavailable");
        }
        if containers.iter().all(|row| row["image"] == id) {
            Ok((None, "no changes".into()))
        } else {
            Ok((Some(id), "new image available".into()))
        }
    }
    fn docker(&self) -> Result<()> {
        let config: Value = serde_json::from_str(
            &self
                .cmd(&self.compose(&["config", "--format", "json"])?, 120, &[0])?
                .stdout,
        )?;
        let project = text(&self.policy["updates"], "project")?;
        if config["name"] != project {
            bail!("unexpected Compose project");
        }
        let before = self.containers()?;
        receipt(
            &self.record.join("containers-before.json"),
            &serde_json::to_value(&before)?,
        )?;
        if before.is_empty() {
            report("SKIP", "Docker images", "no running services");
            return Ok(());
        }
        let workers = self.policy["updates"]["image_workers"]
            .as_u64()
            .unwrap_or(3)
            .clamp(1, 16) as usize;
        report(
            "SYNC",
            "Docker images",
            &format!("checking images; {workers} workers"),
        );
        let work: Vec<_> = before
            .iter()
            .filter(|(name, _)| config["services"].get(*name).is_some())
            .collect();
        let next = AtomicUsize::new(0);
        let mut changed = BTreeMap::new();
        std::thread::scope(|scope| {
            let (sender, receiver) = std::sync::mpsc::channel();
            for _ in 0..workers.min(work.len()) {
                let sender = sender.clone();
                let work = &work;
                let next = &next;
                let config = &config;
                scope.spawn(move || loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((name, containers)) = work.get(i) else {
                        break;
                    };
                    let result =
                        self.prepare(name, &config["services"][*name], containers, project);
                    if sender.send(((*name).clone(), result)).is_err() {
                        break;
                    }
                });
            }
            drop(sender);
            for (name, result) in receiver {
                match result {
                    Err(error) => report("WARN", &name, &format!("keeping current image; {error}")),
                    Ok((None, note)) => report("SKIP", &name, &note),
                    Ok((Some(id), note)) => {
                        report("CHECK", &name, &note);
                        changed.insert(name, id);
                    }
                }
            }
        });
        if changed.is_empty() || !self.backup_ready()? {
            return Ok(());
        }
        storage::check_mounts(self.policy, &storage::config(self.policy)?, true)?;
        let override_path = self.record.join("images.json");
        let services: serde_json::Map<_, _> = changed
            .iter()
            .map(|(name, id)| (name.clone(), json!({"image":id})))
            .collect();
        receipt(&override_path, &json!({"services":services}))?;
        for (name, id) in changed {
            if self
                .deadline
                .saturating_duration_since(Instant::now())
                .as_secs()
                < 210
            {
                bail!("insufficient time for recreation; retry next run");
            }
            let current = self.containers()?;
            let identities = |rows: Option<&Vec<Value>>| -> BTreeSet<String> {
                rows.into_iter()
                    .flatten()
                    .filter_map(|row| row["id"].as_str().map(str::to_owned))
                    .collect()
            };
            if identities(current.get(&name)) != identities(before.get(&name)) {
                bail!("{name}: container state changed; deferred");
            }
            let old_ids: BTreeSet<_> = before[&name]
                .iter()
                .filter_map(|r| r["image"].as_str())
                .collect();
            let mut old: Vec<_> = old_ids.iter().map(|id| self.image_version(id)).collect();
            let mut new = self.image_version(&id);
            if old.contains(&new) {
                old = old_ids
                    .iter()
                    .map(|id| id.trim_start_matches("sha256:").chars().take(12).collect())
                    .collect();
                new = id.trim_start_matches("sha256:").chars().take(12).collect();
            }
            let subject = format!("{name} ({})", old.join(", "));
            report("UPDATE", &subject, "recreating");
            self.cmd(
                &self.compose(&[
                    "-f",
                    override_path.to_str().unwrap(),
                    "up",
                    "-d",
                    "--no-deps",
                    "--no-build",
                    "--pull",
                    "never",
                    "--wait",
                    "--wait-timeout",
                    "180",
                    &name,
                ])?,
                210,
                &[0],
            )?;
            let after = self.containers()?;
            let rows = after.get(&name).context("updated service is not running")?;
            if rows.len() != before[&name].len() || rows.iter().any(|r| r["image"] != id) {
                bail!("running image does not match update");
            }
            report("UPDATE", &subject, &new);
        }
        Ok(())
    }
}
pub fn run(policy: &Value) -> Result<i32> {
    let p = &policy["updates"];
    let mut locks = Vec::new();
    for key in ["lock", "backup_lock"] {
        match crate::process::Lock::acquire(Path::new(text(p, key)?)) {
            Ok(lock) => locks.push(lock),
            Err(e) => {
                if e.to_string().starts_with("workflow/repository is busy") {
                    report(
                        "SKIP",
                        "App updates",
                        "another update, backup or restore is running",
                    );
                    return Ok(0);
                }
                return Err(e);
            }
        }
    }
    let state = Path::new(text(p, "state")?);
    private_dir(state)?;
    let record = state.join(stamp());
    private_dir(&record)?;
    let updater = Updater {
        policy,
        record,
        deadline: Instant::now()
            + Duration::from_secs(p["timeout_seconds"].as_u64().unwrap_or(1200)),
        sequence: AtomicUsize::new(0),
    };
    let mut failed = false;
    for (section, subject, docker) in [
        ("Docker services", "Docker images", true),
        ("Native applications", "Plex", false),
    ] {
        report("SECTION", section, "begin");
        if let Err(error) = if docker {
            updater.docker()
        } else {
            updater.plex()
        } {
            failed = true;
            report("FAILED", subject, &error.to_string());
        }
    }
    Ok(i32::from(failed))
}

fn session_count(text: &str) -> Result<u64> {
    let mut reader = quick_xml::Reader::from_str(text);
    let (mut depth, mut children, mut count) = (0, 0, None);
    loop {
        let event = reader.read_event()?;
        let empty = matches!(&event, quick_xml::events::Event::Empty(_));
        match event {
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                if depth == 0 {
                    if e.name().as_ref() != b"MediaContainer" || count.is_some() {
                        bail!("invalid Plex session response");
                    }
                    for attr in e.attributes() {
                        let attr = attr?;
                        if attr.key.as_ref() == b"size" {
                            count = Some(attr.unescape_value()?.parse::<u64>()?);
                        }
                    }
                } else if depth == 1 {
                    children += 1;
                }
                if !empty {
                    depth += 1;
                }
            }
            quick_xml::events::Event::End(_) => {
                if depth == 0 {
                    bail!("invalid Plex XML nesting");
                }
                depth -= 1;
            }
            quick_xml::events::Event::Eof => break,
            _ => (),
        }
    }
    let count = count.context("Plex session count missing")?;
    if count != children || depth != 0 {
        bail!("inconsistent Plex session count");
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plex_sessions_fail_closed_on_invalid_or_inconsistent_xml() {
        assert_eq!(session_count("<MediaContainer size=\"0\"/>").unwrap(), 0);
        assert_eq!(
            session_count("<MediaContainer size=\"1\"><Video><Player/></Video></MediaContainer>")
                .unwrap(),
            1
        );
        for xml in [
            "<MediaContainer/>",
            "<MediaContainer size=\"0\"><Video/></MediaContainer>",
            "<MediaContainer size=\"2\"/>",
            "<MediaContainer size=\"0\">",
            "<Other size=\"0\"/>",
            "<MediaContainer size=\"0\"/><MediaContainer size=\"0\"/>",
        ] {
            assert!(session_count(xml).is_err(), "{xml}");
        }
    }
}
