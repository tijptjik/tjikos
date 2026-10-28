use crate::{
    archive, fs as tree,
    merge::{self, Entries},
    store::Store,
    util::*,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{symlink, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::Command,
};
pub struct Engine {
    pub config: Value,
    pub home: PathBuf,
    pub machine: String,
    pub master: String,
    pub state: PathBuf,
    pub store: Store,
}
pub type Reporter<'a> = dyn FnMut(&str, &Value) + 'a;
impl Engine {
    pub fn new(config: Value, home: PathBuf, machine: String, state: PathBuf) -> Result<Self> {
        ensure!(config["version"] == 2, "Unsupported archive policy version");
        host(&machine)?;
        let master = text(&config, "master")?.to_owned();
        host(&master)?;
        let remote = text(&config, "remote")?.to_owned();
        private_dir(&state)?;
        let app = Self {
            config,
            home: home.canonicalize()?,
            machine,
            master,
            state,
            store: Store { root: remote },
        };
        for (n, p) in app.config["folders"]
            .as_object()
            .context("Missing folder policy")?
        {
            safe_name(n)?;
            ensure!(
                ["sync", "master", "device", "desktop", "master-backup"].contains(&mode(p)),
                "Unknown folder mode: {n}"
            );
            if let Some(v) = p.get("devices") {
                app.devices(&strings(v)?)?;
            }
            if let Some(v) = p.get("retention") {
                ensure!(
                    ["latest", "daily-weekly-monthly"].contains(&v.as_str().unwrap_or("")),
                    "Unknown retention"
                );
            }
            if let Some(v) = p.get("include") {
                for n in strings(v)? {
                    safe_name(&n)?;
                }
            }
        }
        Ok(app)
    }
    pub fn lock(&self) -> Result<File> {
        let f = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.state.join("lock"))?;
        ensure!(
            unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "Another archive operation is running"
        );
        Ok(f)
    }
    pub fn policy(&self, folder: &str) -> Result<&Value> {
        safe_name(folder)?;
        self.config["folders"]
            .get(folder)
            .context("Folder is not configured")
    }
    pub fn devices(&self, selectors: &[String]) -> Result<Vec<String>> {
        ensure!(!selectors.is_empty(), "Choose a device");
        let mut out = BTreeSet::new();
        for s in selectors {
            if let Some(v) = self.config["device_groups"].get(s) {
                let group = strings(v)?;
                ensure!(!group.is_empty(), "Empty device group");
                for m in group {
                    host(&m)?;
                    out.insert(m);
                }
            } else {
                host(s)?;
                out.insert(s.clone());
            }
        }
        Ok(out.into_iter().collect())
    }
    pub fn device(&self, s: &str) -> Result<String> {
        let d = self.devices(&[s.into()])?;
        ensure!(d.len() == 1, "Choose one device, not a group");
        Ok(d[0].clone())
    }
    pub fn eligible(&self, f: &str, m: &str) -> Result<bool> {
        let p = self.policy(f)?;
        if let Some(v) = p.get("devices") {
            if !self.devices(&strings(v)?)?.contains(&m.to_owned()) {
                return Ok(false);
            }
        }
        Ok(!["desktop", "master-backup"].contains(&mode(p)) || m == self.master)
    }
    pub fn folders(&self, sync: bool) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for (n, p) in self.config["folders"].as_object().unwrap() {
            if self.eligible(n, &self.machine)? && (!sync || ["sync", "master"].contains(&mode(p)))
            {
                out.push(n.clone())
            }
        }
        Ok(out)
    }
    pub fn prefix(&self, f: &str, m: &str) -> Result<String> {
        self.policy(f)?;
        host(m)?;
        Ok(format!("{m}/{f}"))
    }
    pub fn baseline(&self, f: &str, m: &str) -> PathBuf {
        self.state
            .join("sync")
            .join(m)
            .join(format!("{}.json", digest(&json!(f))))
    }
    pub fn ready(&self) -> Result<()> {
        if self.machine == self.master {
            return Ok(());
        }
        if let Some(v) = self.config.get("initial_sync_required") {
            let s = strings(v)?;
            if s.is_empty() || !self.devices(&s)?.contains(&self.machine) {
                return Ok(());
            }
        }
        let source = self.config["bootstrap_source"]
            .as_str()
            .unwrap_or(&self.master);
        let mut missing = Vec::new();
        for f in self.folders(self.config.get("bootstrap_source").is_none())? {
            let receipt = optional_json(&self.baseline(&f, source))?;
            if receipt["folder"] != f
                || receipt["source"] != source
                || receipt["version"] != 2
                || !receipt["entries"].is_object()
            {
                missing.push(f);
            }
        }
        ensure!(
            missing.is_empty(),
            "Initial restoration incomplete: {}",
            missing.join(", ")
        );
        Ok(())
    }
    fn encode(&self, src: &Path, dst: &Path, encrypted: bool) -> Result<()> {
        if encrypted {
            run(Command::new("age")
                .args(["-r", text(&self.config, "recipient")?, "-o"])
                .arg(dst)
                .arg(src))
        } else {
            tree::copy_regular(src, dst)
        }
    }
    fn decode(&self, src: &Path, dst: &Path, encrypted: bool) -> Result<()> {
        if encrypted {
            run(Command::new("age")
                .args(["-d", "-i"])
                .arg(expand(text(&self.config, "identity")?, &self.home))
                .arg("-o")
                .arg(dst)
                .arg(src))
        } else {
            tree::copy_regular(src, dst)
        }
    }
    pub fn fetch(
        &self,
        f: &str,
        source: &str,
        dest: &Path,
        snap: Option<&str>,
        tree: bool,
    ) -> Result<(Value, Value)> {
        let prefix = self.prefix(f, source)?;
        let key = if let Some(s) = snap {
            snapshot(s)?;
            format!("{prefix}/{s}/complete.json")
        } else {
            format!("{prefix}/latest.json")
        };
        self.store.get(&key, &dest.join("ref.json"))?;
        let r = read_json(&dest.join("ref.json"))?;
        ensure!(
            r["version"] == 2 && r["folder"] == f && r["machine"] == source,
            "Snapshot identity mismatch"
        );
        let s = text(&r, "snapshot")?;
        snapshot(s)?;
        if let Some(wanted) = snap {
            ensure!(s == wanted, "Pinned snapshot identity mismatch");
        }
        // Latest is never trusted without the matching completion marker.
        if snap.is_none() {
            self.store.get(
                &format!("{prefix}/{s}/complete.json"),
                &dest.join("complete.json"),
            )?;
            ensure!(
                read_json(&dest.join("complete.json"))? == r,
                "Snapshot is not complete"
            );
        }
        let suffix = if encrypted(&r) { ".age" } else { "" };
        let mut names = vec![("manifest.json", "manifest_sha256")];
        if tree {
            names.push(("tree.tar.zst", "archive_sha256"));
        }
        for (name, key) in names {
            let downloaded = dest.join(format!("download-{name}{suffix}"));
            self.store
                .get(&format!("{prefix}/{s}/{name}{suffix}"), &downloaded)?;
            ensure!(
                hash(&downloaded)? == text(&r, key)?,
                "Snapshot checksum mismatch"
            );
            self.decode(&downloaded, &dest.join(name), encrypted(&r))?;
        }
        let m = read_json(&dest.join("manifest.json"))?;
        for k in ["version", "folder", "machine", "snapshot"] {
            ensure!(m[k] == r[k], "Manifest identity mismatch");
        }
        ensure!(m["policy"].is_object(), "Missing snapshot policy");
        let entries = merge::entries(&m["entries"])?;
        tree::check_tree(&entries)?;
        ensure!(
            m["root_mode"].as_u64().is_some_and(|m| m <= 0o777),
            "Invalid root permissions"
        );
        if tree {
            archive::unpack(&dest.join("tree.tar.zst"), &entries, &dest.join("tree"))?;
        }
        Ok((r, m))
    }
    pub fn backup(&self, f: &str, force: bool, report: &mut Reporter) -> Result<Value> {
        self.ready()?;
        ensure!(
            self.eligible(f, &self.machine)?,
            "Folder not assigned to this device"
        );
        let p = self.policy(f)?;
        let root = tree::safe_root(&self.home, f)?;
        let tmp = tempfile::tempdir_in(&self.state)?;
        let t = tmp.path();
        let prefix = self.prefix(f, &self.machine)?;
        let hashname = format!("{}.json", digest(&json!(f)));
        let last = self.state.join("manifests").join(&hashname);
        let receipt = self.state.join("backups").join(&hashname);
        let previous = optional_json(&last)?;
        let prior = optional_json(&receipt)?;
        report("CHECK", &json!({"reason":"compare content hashes"}));
        let entries =
            tree::fingerprint(&root, p, &self.state.join("hash-cache").join(&hashname), t)?;
        let root_mode = fs::metadata(&root)?.mode() & 0o777;
        let mut same = !force
            && previous["machine"] == self.machine
            && previous["folder"] == f
            && previous["snapshot"] == prior["snapshot"]
            && previous["entries"] == json!(entries)
            && previous["root_mode"] == root_mode
            && previous["policy"].is_object()
            && merge::backup_policy(&previous["policy"]) == merge::backup_policy(p)
            && (!encrypted(p)
                || prior.get("recipient").unwrap_or(&self.config["recipient"])
                    == &self.config["recipient"]);
        if same {
            report(
                "CHECK",
                &json!({"entries":entries.len(),"reason":"verify existing remote snapshot"}),
            );
            same = (|| -> Result<bool> {
                let s = text(&prior, "snapshot")?;
                snapshot(s)?;
                self.store
                    .get(&format!("{prefix}/latest.json"), &t.join("latest"))?;
                self.store
                    .get(&format!("{prefix}/{s}/complete.json"), &t.join("complete"))?;
                let names = self.store.list(&format!("{prefix}/{s}"), false)?;
                let suffix = if encrypted(&prior) { ".age" } else { "" };
                Ok(read_json(&t.join("latest"))? == prior
                    && read_json(&t.join("complete"))? == prior
                    && [
                        format!("manifest.json{suffix}"),
                        format!("tree.tar.zst{suffix}"),
                    ]
                    .iter()
                    .all(|n| names.contains(n)))
            })()
            .unwrap_or(false);
        }
        if same {
            if p.get("retention").is_some() {
                self.prune(f, true, Some(&prior))?;
            }
            atomic_json(
                &self.state.join("checks").join(&hashname),
                &json!({"checked_at":now(),"snapshot":prior["snapshot"],"unchanged":true}),
            )?;
            report(
                "SKIP",
                &json!({"entries":entries.len(),"reason":"unchanged"}),
            );
            let mut result = prior;
            result["skipped"] = json!(true);
            return Ok(result);
        }
        report("STAGE", &json!({"reason":"snapshot and verify source"}));
        let (entries, dbs) = tree::stage(&root, &t.join("tree"), p)?;
        let snap = format!(
            "{}-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%S.%6fZ"),
            &id()[..12]
        );
        let before = previous["entries"].as_object();
        let added: Vec<_> = entries
            .keys()
            .filter(|n| !before.is_some_and(|v| v.contains_key(*n)))
            .collect();
        let removed: Vec<_> = before
            .into_iter()
            .flat_map(|v| v.keys())
            .filter(|n| !entries.contains_key(*n))
            .collect();
        let modified: Vec<_> = entries
            .iter()
            .filter(|(n, v)| before.is_some_and(|b| b.get(*n).is_some_and(|old| old != *v)))
            .map(|(n, _)| n)
            .collect();
        let manifest = json!({"version":2,"machine":self.machine,"folder":f,"snapshot":snap,"created_at":now(),"policy":p,"root_mode":root_mode,"entries":entries,"sqlite":dbs,"previous_snapshot":previous["snapshot"],"changes":{"added":added,"removed":removed,"modified":modified}});
        atomic_json(&t.join("manifest.json"), &manifest)?;
        report("PACK", &json!({"entries":entries.len()}));
        archive::pack(&t.join("tree"), &entries, &t.join("tree.tar.zst"))?;
        let enc = encrypted(p);
        let suffix = if enc { ".age" } else { "" };
        if enc {
            report("ENCRYPT", &json!({}));
            for n in ["manifest.json", "tree.tar.zst"] {
                self.encode(&t.join(n), &t.join(format!("{n}{suffix}")), true)?;
            }
        }
        let r = json!({"version":2,"machine":self.machine,"folder":f,"snapshot":snap,"created_at":manifest["created_at"],"encrypted":enc,"recipient":if enc{self.config["recipient"].clone()}else{Value::Null},"manifest_sha256":hash(&t.join(format!("manifest.json{suffix}")))?,"archive_sha256":hash(&t.join(format!("tree.tar.zst{suffix}")))?});
        let size = fs::metadata(t.join(format!("tree.tar.zst{suffix}")))?.len();
        for (n, k) in [
            ("manifest.json", "manifest_sha256"),
            ("tree.tar.zst", "archive_sha256"),
        ] {
            let name = format!("{n}{suffix}");
            report("UPLOAD", &json!({"size":size,"entries":entries.len()}));
            self.store
                .put(&t.join(&name), &format!("{prefix}/{snap}/{name}"), true)?;
            self.store.get(
                &format!("{prefix}/{snap}/{name}"),
                &t.join(format!("readback-{name}")),
            )?;
            ensure!(
                hash(&t.join(format!("readback-{name}")))? == text(&r, k)?,
                "Remote readback checksum mismatch"
            );
        }
        report("VERIFY", &json!({"size":size,"entries":entries.len()}));
        self.decode(
            &t.join(format!("readback-tree.tar.zst{suffix}")),
            &t.join("verify.tar.zst"),
            enc,
        )?;
        archive::unpack(&t.join("verify.tar.zst"), &entries, &t.join("verified"))?;
        self.decode(
            &t.join(format!("readback-manifest.json{suffix}")),
            &t.join("verify.json"),
            enc,
        )?;
        ensure!(
            read_json(&t.join("verify.json"))? == manifest,
            "Remote manifest differs"
        );
        report("PUBLISH", &json!({"size":size,"entries":entries.len()}));
        atomic_json(&t.join("ref.json"), &r)?;
        self.store.put(
            &t.join("ref.json"),
            &format!("{prefix}/{snap}/complete.json"),
            true,
        )?;
        self.store
            .put(&t.join("ref.json"), &format!("{prefix}/latest.json"), false)?;
        self.store
            .get(&format!("{prefix}/latest.json"), &t.join("published.json"))?;
        ensure!(
            read_json(&t.join("published.json"))? == r,
            "Latest pointer verification failed"
        );
        atomic_json(&receipt, &r)?;
        atomic_json(&last, &manifest)?;
        if p.get("retention").is_some() {
            self.prune(f, true, Some(&r))?;
        }
        report(
            "BACKUP",
            &json!({"entries":entries.len(),"size":size,"reason":"verified"}),
        );
        Ok(r)
    }
    pub fn prune(&self, f: &str, apply: bool, verified: Option<&Value>) -> Result<Value> {
        ensure!(
            self.eligible(f, &self.machine)?,
            "Cannot prune another device"
        );
        let policy = text(self.policy(f)?, "retention")?;
        let prefix = self.prefix(f, &self.machine)?;
        let tmp = tempfile::tempdir_in(&self.state)?;
        let latest = if let Some(v) = verified {
            v.clone()
        } else {
            self.fetch(f, &self.machine, tmp.path(), None, true)?.0
        };
        let check = tmp.path().join("current");
        self.store.get(&format!("{prefix}/latest.json"), &check)?;
        ensure!(
            read_json(&check)? == latest,
            "Latest changed before retention"
        );
        let mut refs = Vec::new();
        for s in self.store.list(&prefix, true)? {
            snapshot(&s)?;
            let path = tmp.path().join(format!("ref-{s}"));
            self.store
                .get(&format!("{prefix}/{s}/complete.json"), &path)?;
            let r = read_json(&path)?;
            ensure!(
                r["snapshot"] == s && r["machine"] == self.machine && r["folder"] == f,
                "Retention identity mismatch"
            );
            refs.push(r);
        }
        let s = text(&latest, "snapshot")?;
        ensure!(
            refs.iter().any(|v| v["snapshot"] == s),
            "Latest is incomplete"
        );
        let keep = merge::retention(&refs, s, policy, chrono::Utc::now())?;
        let remove: Vec<_> = refs
            .iter()
            .map(|v| text(v, "snapshot").unwrap().to_owned())
            .filter(|s| !keep.contains(s))
            .collect();
        let report = json!({"folder":f,"keep":keep,"remove":remove,"applied":apply});
        if apply {
            atomic_json(
                &self
                    .state
                    .join("retention")
                    .join(format!("{}.json", digest(&json!(f)))),
                &report,
            )?;
            for s in remove {
                self.store.remove(&prefix, &s)?;
            }
        }
        Ok(report)
    }
    pub fn plan(&self, f: &str, source: &str, snap: Option<&str>) -> Result<(PathBuf, Value)> {
        let p = self.policy(f)?;
        ensure!(
            self.eligible(f, &self.machine)?,
            "Folder not assigned to device"
        );
        ensure!(source != self.machine, "Choose a different source");
        ensure!(
            ["sync", "master"].contains(&mode(p)),
            "Backup-only collection: restore to a new directory"
        );
        if mode(p) == "master" {
            ensure!(
                source == self.master && self.machine != self.master,
                "Leader mirror accepts only its leader"
            );
        }
        let tmp = tempfile::tempdir_in(&self.state)?;
        let (r, m) = self.fetch(f, source, tmp.path(), snap, false)?;
        ensure!(
            merge::managed(&m["policy"]) == merge::managed(p),
            "Snapshot policy differs; back up under current policy first"
        );
        let prior = optional_json(&self.baseline(f, source))?;
        let base = if prior.is_null() {
            Entries::new()
        } else {
            merge::entries(&prior["entries"])?
        };
        let local = tree::scan(&tree::safe_root(&self.home, f)?, p)?;
        let incoming = merge::entries(&m["entries"])?;
        let (desired, changes, conflicts) = merge::merge(
            &base,
            &local,
            &incoming,
            mode(p) == "master",
            &BTreeMap::new(),
        )?;
        let plan = json!({"version":2,"machine":self.machine,"folder":f,"source":source,"created_at":now(),"ref":r,"policy":p,"baseline_hash":digest(&prior),"local":local,"base":base,"incoming":incoming,"root_mode":m["root_mode"],"changes":changes,"conflicts":conflicts,"resolutions":{},"desired":desired});
        let path = self.state.join("plans").join(format!("{}.json", id()));
        atomic_json(&path, &plan)?;
        Ok((path, plan))
    }
    pub fn apply(&self, path: &Path, resolutions: &BTreeMap<String, String>) -> Result<Value> {
        let plan = read_json(path)?;
        let f = text(&plan, "folder")?;
        let source = text(&plan, "source")?;
        host(source)?;
        let p = self.policy(f)?;
        ensure!(
            plan["version"] == 2 && plan["machine"] == self.machine && plan["policy"] == *p,
            "Plan belongs to another device or policy"
        );
        ensure!(
            self.eligible(f, &self.machine)? && ["sync", "master"].contains(&mode(p)),
            "Folder cannot be synchronized"
        );
        if mode(p) == "master" {
            ensure!(
                source == self.master
                    && self.machine != self.master
                    && resolutions.values().all(|v| v == "source"),
                "Leader mirrors cannot retain local overrides"
            );
        }
        let target = tree::safe_root(&self.home, f)?;
        let baseline = self.baseline(f, source);
        let prior = optional_json(&baseline)?;
        let local = merge::entries(&plan["local"])?;
        let incoming = merge::entries(&plan["incoming"])?;
        let base = merge::entries(&plan["base"])?;
        ensure!(
            digest(&prior) == text(&plan, "baseline_hash")? && tree::scan(&target, p)? == local,
            "Local files or baseline changed; create a new plan"
        );
        ensure!(
            base == if prior.is_null() {
                Entries::new()
            } else {
                merge::entries(&prior["entries"])?
            },
            "Plan baseline does not match receipt"
        );
        for n in resolutions.keys() {
            ensure!(
                local.get(n) != incoming.get(n)
                    || plan["conflicts"]
                        .as_array()
                        .is_some_and(|v| v.contains(&json!(n))),
                "Resolution is not a differing path"
            );
        }
        let (desired, _, conflicts) =
            merge::merge(&base, &local, &incoming, mode(p) == "master", resolutions)?;
        ensure!(
            conflicts.is_empty(),
            "Review required for {} conflicting paths",
            conflicts.len()
        );
        tree::check_tree(&desired)?;
        tree::no_writers(&target)?;
        let parent = target.parent().context("Missing parent")?;
        fs::create_dir_all(parent)?;
        let recovery = parent.join(format!(
            "{}.archive-recovery-{}",
            target.file_name().unwrap().to_string_lossy(),
            id()
        ));
        let work = tempfile::Builder::new()
            .prefix(".archive-apply-")
            .tempdir_in(parent)?;
        let source_dir = work.path().join("source");
        fs::create_dir(&source_dir)?;
        let (r, m) = self.fetch(
            f,
            source,
            &source_dir,
            Some(text(&plan["ref"], "snapshot")?),
            true,
        )?;
        ensure!(
            r == plan["ref"]
                && m["entries"] == plan["incoming"]
                && m["root_mode"] == plan["root_mode"],
            "Snapshot changed since planning"
        );
        let new = work.path().join("new");
        if target.exists() {
            tree::copy_tree(&target, &new)?;
        } else {
            fs::create_dir(&new)?;
        }
        let mut names: Vec<_> = local.keys().collect();
        names.sort_by_key(|n| (std::cmp::Reverse(n.matches('/').count()), *n));
        for n in names {
            if local.get(n) == desired.get(n) {
                continue;
            }
            let q = new.join(n);
            if q.is_dir() && !q.is_symlink() {
                if desired.get(n).is_some_and(|e| e["type"] == "dir") {
                    continue;
                }
                fs::remove_dir(&q).context("Excluded/local files prevent directory replacement")?;
            } else if q.exists() || q.is_symlink() {
                fs::remove_file(q)?;
            }
        }
        for (n, e) in &desired {
            let q = new.join(n);
            if e["type"] == "dir" {
                if !q.exists() {
                    fs::create_dir(&q)?;
                }
            } else if local.get(n) != Some(e) {
                ensure!(incoming.get(n) == Some(e), "Unknown content in plan");
                if e["type"] == "link" {
                    symlink(text(e, "target")?, &q)?;
                } else {
                    tree::copy_regular(&source_dir.join("tree").join(n), &q)?;
                }
            }
            if e["type"] != "link" {
                fs::set_permissions(
                    q,
                    fs::Permissions::from_mode(e["mode"].as_u64().unwrap() as u32),
                )?;
            }
        }
        fs::set_permissions(
            &new,
            fs::Permissions::from_mode(
                m["root_mode"].as_u64().context("Invalid permissions")? as u32
            ),
        )?;
        ensure!(
            tree::scan(&new, p)? == desired && tree::scan(&target, p)? == local,
            "Files changed during apply; destination preserved"
        );
        let journal = self
            .state
            .join("transactions")
            .join(format!("{}.json", id()));
        let mut receipt = json!({"folder":f,"source":source,"snapshot":r["snapshot"],"recovery":recovery,"target":target,"status":"prepared","created_at":now()});
        atomic_json(&journal, &receipt)?;
        tree::no_writers(&target)?;
        ensure!(
            tree::scan(&target, p)? == local,
            "Destination changed before swap"
        );
        let existed = target.exists();
        if existed {
            fs::rename(&target, &recovery)?;
        }
        let result = (|| -> Result<()> {
            fs::rename(&new, &target)?;
            if Path::new("/sys/fs/selinux/enforce").exists() {
                run(Command::new("restorecon").arg("-R").arg(&target))?;
            }
            ensure!(
                tree::scan(&target, p)? == desired,
                "Post-apply verification failed"
            );
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if let Err(error) = result {
            if target.exists() {
                fs::rename(&target, work.path().join("failed"))?;
            }
            if existed {
                fs::rename(&recovery, &target)?;
            }
            receipt["status"] = json!("rolled-back");
            atomic_json(&journal, &receipt)?;
            return Err(error);
        }
        atomic_json(
            &baseline,
            &json!({"version":2,"folder":f,"source":source,"snapshot":r["snapshot"],"last_synced_at":now(),"entries":m["entries"]}),
        )?;
        receipt["status"] = json!("complete");
        atomic_json(&journal, &receipt)?;
        Ok(receipt)
    }
    pub fn restore(&self, f: &str, source: &str, dest: &Path, snap: Option<&str>) -> Result<Value> {
        let p = self.policy(f)?;
        if ["master", "desktop", "master-backup"].contains(&mode(p)) {
            ensure!(source == self.master, "Restore must use the leader");
        }
        ensure!(dest.is_absolute(), "Restore destination must be absolute");
        ensure!(
            !dest.exists() && !dest.is_symlink(),
            "Restore destination must not exist"
        );
        let parent = dest.parent().context("Missing parent")?;
        fs::create_dir_all(parent)?;
        ensure!(
            parent.canonicalize()? == parent,
            "Restore destination has symlinked parents"
        );
        let temp = tempfile::Builder::new()
            .prefix(".archive-restore-")
            .tempdir_in(parent)?;
        let (r, m) = self.fetch(f, source, temp.path(), snap, true)?;
        let staged = temp.path().join("tree");
        fs::set_permissions(
            &staged,
            fs::Permissions::from_mode(m["root_mode"].as_u64().unwrap() as u32),
        )?;
        ensure!(
            tree::scan(&staged, &json!({}))? == merge::entries(&m["entries"])?,
            "Restore verification failed"
        );
        // RENAME_NOREPLACE prevents a concurrent creator being overwritten.
        let a = std::ffi::CString::new(staged.as_os_str().as_encoded_bytes())?;
        let b = std::ffi::CString::new(dest.as_os_str().as_encoded_bytes())?;
        ensure!(
            unsafe {
                libc::syscall(
                    libc::SYS_renameat2,
                    libc::AT_FDCWD,
                    a.as_ptr(),
                    libc::AT_FDCWD,
                    b.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            } == 0,
            "Restore destination changed or could not be installed"
        );
        File::open(parent)?.sync_all()?;
        Ok(r)
    }
    pub fn status(&self, selectors: &[String]) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        for (f, p) in self.config["folders"].as_object().unwrap() {
            if !selectors.is_empty()
                && !self
                    .devices(selectors)?
                    .iter()
                    .any(|d| self.eligible(f, d).unwrap_or(false))
            {
                continue;
            }
            let mut receipts = Vec::new();
            if let Ok(dirs) = fs::read_dir(self.state.join("sync")) {
                for dir in dirs {
                    let path = dir?.path().join(format!("{}.json", digest(&json!(f))));
                    if path.is_file() {
                        let mut v = read_json(&path)?;
                        v["entries"] = json!("(manifest retained locally)");
                        receipts.push(v);
                    }
                }
            }
            out.push(json!({"folder":f,"mode":p["mode"],"devices":p["devices"],"backup_here":self.eligible(f,&self.machine)?,"backup":optional_json(&self.state.join("backups").join(format!("{}.json",digest(&json!(f)))))?,"sync":receipts}));
        }
        Ok(out)
    }
}
