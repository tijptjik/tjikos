use crate::{merge::Entries, util::*};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{symlink, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    time::{Duration, Instant},
};
fn matches(p: &str, s: &str) -> Result<bool> {
    Ok(glob::Pattern::new(p)?.matches_with(
        s,
        glob::MatchOptions {
            case_sensitive: true,
            require_literal_separator: false,
            require_literal_leading_dot: false,
        },
    ))
}
fn selected(name: &str, patterns: &[String], directory: bool) -> Result<bool> {
    let parts: Vec<_> = name.split('/').collect();
    for pattern in patterns {
        let mut fixed: Vec<_> = pattern.split('/').collect();
        let recursive = fixed.last() == Some(&"**");
        if recursive {
            fixed.pop();
        }
        let length = if directory {
            recursive || parts.len() < fixed.len()
        } else {
            (recursive && parts.len() >= fixed.len()) || parts.len() == fixed.len()
        };
        if length {
            let mut yes = true;
            for (a, b) in parts.iter().zip(&fixed) {
                yes &= matches(b, a)?;
            }
            if yes {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn excluded(name: &str, p: &Value) -> Result<bool> {
    if let Some(include) = p.get("include") {
        let items = strings(include)?;
        if !items.is_empty()
            && !items
                .iter()
                .any(|x| name == x || name.starts_with(&format!("{x}/")))
        {
            return Ok(true);
        }
    }
    if let Some(ex) = p.get("exclude") {
        for pat in strings(ex)? {
            if matches(&pat, name)? {
                return Ok(true);
            }
            for part in name.split('/') {
                if matches(&pat, part)? {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}
pub fn check_tree(e: &Entries) -> Result<()> {
    for (n, v) in e {
        safe_name(n)?;
        ensure!(
            ["dir", "file", "link"].contains(&text(v, "type")?),
            "Unsupported entry"
        );
        ensure!(
            v["mode"].as_u64().is_some_and(|m| m <= 0o777),
            "Invalid permissions"
        );
        if v["type"] == "file" {
            ensure!(v["size"].as_u64().is_some(), "Invalid size");
            let h = text(v, "sha256")?;
            ensure!(
                h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()),
                "Invalid content hash"
            );
        }
        if v["type"] == "link" {
            ensure!(!text(v, "target")?.contains('\0'), "Invalid link");
        }
        let mut s = n.as_str();
        while let Some((p, _)) = s.rsplit_once('/') {
            ensure!(
                e.get(p).is_some_and(|v| v["type"] == "dir"),
                "Non-directory manifest parent: {p}"
            );
            s = p;
        }
    }
    Ok(())
}
pub fn safe_root(home: &Path, folder: &str) -> Result<std::path::PathBuf> {
    safe_name(folder)?;
    let mut root = home.to_path_buf();
    for part in folder.split('/') {
        root.push(part);
        match fs::symlink_metadata(&root) {
            Ok(m) => ensure!(
                m.is_dir() && !m.file_type().is_symlink(),
                "Folder path contains a symlink or non-directory: {}",
                root.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(root)
}
pub fn is_sqlite(p: &Path) -> Result<bool> {
    let mut b = [0; 16];
    let n = read_file(p)?.read(&mut b)?;
    Ok(n == 16 && &b == b"SQLite format 3\0")
}
pub fn scan(root: &Path, p: &Value) -> Result<Entries> {
    scan_using(root, p, &mut |path, m| {
        Ok(json!({"size":m.len(),"sha256":hash(path)?}))
    })
}
fn scan_using(
    root: &Path,
    p: &Value,
    entry: &mut dyn FnMut(&Path, &fs::Metadata) -> Result<Value>,
) -> Result<Entries> {
    let mut policy = p.clone();
    let mut required = Entries::new();
    if p["telegram_themes"] == true {
        ensure!(
            ["device", "desktop", "master-backup"].contains(&mode(p))
                && encrypted(p)
                && p["include_files"].is_array(),
            "Telegram recovery requires encrypted allowlisted backup-only mode"
        );
        let (hashes, _) = crate::telegram::recovery(root)?;
        let mut include = strings(&p["include_files"])?;
        for (n, h) in hashes {
            include.push(n.clone());
            required.insert(n, json!(h));
        }
        policy["include_files"] = json!(include);
    }
    fn walk(
        root: &Path,
        dir: &Path,
        p: &Value,
        out: &mut Entries,
        entry: &mut dyn FnMut(&Path, &fs::Metadata) -> Result<Value>,
    ) -> Result<()> {
        let mut paths = fs::read_dir(dir)?
            .map(|x| x.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        for path in paths {
            let name = path
                .strip_prefix(root)?
                .to_str()
                .context("Non-UTF8 filename")?
                .to_owned();
            if excluded(&name, p)? {
                continue;
            }
            let m = fs::symlink_metadata(&path)?;
            let t = m.file_type();
            let perms = m.mode() & 0o777;
            let allow = p
                .get("include_files")
                .map(strings)
                .transpose()?
                .unwrap_or_default();
            if !allow.is_empty() && !selected(&name, &allow, t.is_dir())? {
                continue;
            }
            if t.is_symlink() {
                out.insert(name,json!({"type":"link","mode":perms,"target":fs::read_link(&path)?.to_str().context("Non-UTF8 link")?}));
            } else if t.is_dir() {
                out.insert(name.clone(), json!({"type":"dir","mode":perms}));
                walk(root, &path, p, out, entry)?;
                if !allow.is_empty() && !out.keys().any(|k| k.starts_with(&format!("{name}/"))) {
                    out.remove(&name);
                }
            } else if t.is_file() {
                let mut journal = false;
                for suffix in ["-wal", "-shm", "-journal"] {
                    if let Some(db) = name.strip_suffix(suffix) {
                        let f = root.join(db);
                        if f.is_file() && !f.is_symlink() && is_sqlite(&f)? {
                            journal = true;
                        }
                    }
                }
                if journal {
                    continue;
                }
                let value = entry(&path, &m)?;
                let after = fs::symlink_metadata(&path)?;
                ensure!(
                    signature(&m) == signature(&after),
                    "File changed during scan"
                );
                out.insert(
                    name,
                    json!({"type":"file","mode":perms,"size":value["size"],"sha256":value["sha256"]}),
                );
            } else if !t.is_socket() {
                bail!("Unsupported special file: {}", path.display())
            }
        }
        Ok(())
    }
    let mut out = Entries::new();
    match fs::symlink_metadata(root) {
        Ok(m) => {
            ensure!(
                m.is_dir() && !m.file_type().is_symlink(),
                "Refusing non-directory archive root"
            );
            walk(root, root, &policy, &mut out, entry)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    for (n, h) in required {
        ensure!(
            out.get(&n).is_some_and(|e| e["sha256"] == h),
            "Telegram dependency changed or excluded"
        );
    }
    Ok(out)
}
fn signature(m: &fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64, u32) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
        m.mode(),
    )
}
pub fn copy_regular(src: &Path, dst: &Path) -> Result<()> {
    let mut input = read_file(src)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dst)?;
    std::io::copy(&mut input, &mut output)?;
    output.flush()?;
    Ok(())
}
pub fn sqlite_copy(src: &Path, dst: &Path, options: Option<&Value>) -> Result<()> {
    use rusqlite::{Connection, OpenFlags};
    ensure!(!dst.exists(), "SQLite target exists");
    let source = Connection::open_with_flags(
        src,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut target = Connection::open(dst)?;
    source.busy_timeout(Duration::from_secs(5))?;
    {
        let backup = rusqlite::backup::Backup::new(&source, &mut target)?;
        let start = Instant::now();
        loop {
            ensure!(
                start.elapsed() < Duration::from_secs(120),
                "SQLite snapshot timed out"
            );
            match backup.step(1024)? {
                rusqlite::backup::StepResult::Done => break,
                rusqlite::backup::StepResult::More => (),
                _ => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }
    target.execute_batch("PRAGMA journal_mode=DELETE;")?;
    if let Some(p) = options {
        for k in p.as_object().context("Invalid SQLite options")?.keys() {
            ensure!(
                ["omit_tables", "max_bytes"].contains(&k.as_str()),
                "Unknown SQLite recovery option"
            );
        }
        let omit = p
            .get("omit_tables")
            .map(strings)
            .transpose()?
            .unwrap_or_default();
        for n in &omit {
            ensure!(
                !n.starts_with("sqlite_")
                    && !n.is_empty()
                    && (n.as_bytes()[0].is_ascii_alphabetic() || n.starts_with('_'))
                    && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                "Invalid SQLite table name"
            );
            let exists: i64 = target.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name=?1",
                [n],
                |r| r.get(0),
            )?;
            ensure!(exists == 1, "SQLite recovery schema changed");
            let triggers: i64 = target.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type='trigger' AND tbl_name=?1",
                [n],
                |r| r.get(0),
            )?;
            ensure!(triggers == 0, "Refusing deletion triggers");
        }
        if !omit.is_empty() {
            target.execute_batch("PRAGMA foreign_keys=OFF;")?;
            let tx = target.transaction()?;
            let seq: i64 = tx.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='sqlite_sequence'",
                [],
                |r| r.get(0),
            )?;
            for n in &omit {
                tx.execute(&format!("DELETE FROM \"{n}\""), [])?;
                if seq > 0 {
                    tx.execute("DELETE FROM sqlite_sequence WHERE name=?1", [n])?;
                }
            }
            tx.commit()?;
            ensure!(
                !target
                    .prepare("PRAGMA foreign_key_check")?
                    .query([])?
                    .next()?
                    .is_some(),
                "SQLite recovery breaks foreign keys"
            );
            target.execute_batch("VACUUM;")?;
        }
        if let Some(limit) = p.get("max_bytes") {
            let n = limit
                .as_u64()
                .filter(|n| *n > 0)
                .context("Invalid SQLite size limit")?;
            ensure!(
                fs::metadata(dst)?.len() <= n,
                "SQLite recovery exceeds size limit"
            );
        }
    }
    let checks: Vec<String> = target
        .prepare("PRAGMA quick_check")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    ensure!(checks == ["ok"], "SQLite validation failed");
    Ok(())
}
pub fn stage(src: &Path, dst: &Path, p: &Value) -> Result<(Entries, Vec<String>)> {
    ensure!(
        src.is_dir() && !src.is_symlink(),
        "Missing or symlinked source"
    );
    let before = scan(src, p)?;
    fs::create_dir(dst)?;
    let mut dbs = BTreeSet::new();
    for (n, e) in &before {
        let a = src.join(n);
        let b = dst.join(n);
        match text(e, "type")? {
            "dir" => fs::create_dir(&b)?,
            "link" => symlink(text(e, "target")?, &b)?,
            _ => {
                if is_sqlite(&a)? {
                    dbs.insert(n.clone());
                    let options = p.get("sqlite_recovery").and_then(|v| v.get(n));
                    if options.is_some() {
                        ensure!(
                            ["device", "desktop", "master-backup"].contains(&mode(p))
                                && encrypted(p)
                                && strings(&p["sqlite"])?.contains(n),
                            "SQLite recovery requires encrypted backup-only mode"
                        );
                    }
                    sqlite_copy(&a, &b, options)?;
                } else {
                    copy_regular(&a, &b)?;
                }
                fs::set_permissions(
                    &b,
                    fs::Permissions::from_mode(e["mode"].as_u64().unwrap() as u32),
                )?;
            }
        }
    }
    let after = scan(src, p)?;
    ensure!(
        before.keys().eq(after.keys())
            && before
                .iter()
                .filter(|(n, _)| !dbs.contains(*n))
                .all(|(n, e)| after.get(n) == Some(e)),
        "Files changed during backup"
    );
    let mut compact = BTreeSet::new();
    if p["telegram_themes"] == true {
        let (required, contents) = crate::telegram::recovery(dst)?;
        for (n, h) in required {
            ensure!(before[&n]["sha256"] == h, "Telegram changed during staging");
        }
        for (n, data) in contents {
            fs::write(dst.join(&n), data)?;
            compact.insert(n);
        }
    }
    for (n, e) in before.iter().rev() {
        if e["type"] == "dir" {
            fs::set_permissions(
                dst.join(n),
                fs::Permissions::from_mode(e["mode"].as_u64().unwrap() as u32),
            )?;
        }
    }
    let staged = scan(dst, &json!({}))?;
    ensure!(
        before
            .iter()
            .filter(|(n, _)| !dbs.contains(*n) && !compact.contains(*n))
            .all(|(n, e)| staged.get(n) == Some(e)),
        "Staging verification failed"
    );
    for n in p
        .get("sqlite")
        .map(strings)
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .chain(
            p.get("sqlite_recovery")
                .and_then(|v| v.as_object())
                .into_iter()
                .flat_map(|v| v.keys().cloned()),
        )
    {
        ensure!(dbs.contains(&n), "Required SQLite database missing: {n}");
    }
    for n in p
        .get("include")
        .map(strings)
        .transpose()?
        .unwrap_or_default()
    {
        ensure!(staged.contains_key(&n), "Required file missing: {n}");
    }
    Ok((staged, dbs.into_iter().collect()))
}
pub fn no_writers(root: &Path) -> Result<()> {
    let prefix = format!("{}/", root.display());
    for entry in fs::read_dir("/proc")? {
        let p = entry?.path();
        if !p
            .file_name()
            .unwrap()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
            || p.file_name().unwrap() == std::process::id().to_string().as_str()
        {
            continue;
        }
        if fs::metadata(&p)
            .map(|m| m.uid() != unsafe { libc::getuid() })
            .unwrap_or(true)
        {
            continue;
        }
        if let Ok(fds) = fs::read_dir(p.join("fd")) {
            for fd in fds.flatten() {
                let Ok(link) = fs::read_link(fd.path()) else {
                    continue;
                };
                if !link.to_string_lossy().starts_with(&prefix) {
                    continue;
                }
                if let Ok(info) = fs::read_to_string(p.join("fdinfo").join(fd.file_name())) {
                    if let Some(flags) = info.lines().find_map(|s| s.strip_prefix("flags:")) {
                        let f = u32::from_str_radix(flags.trim(), 8)?;
                        ensure!(
                            f & libc::O_ACCMODE as u32 == 0,
                            "Close the application writing this folder before restoration"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
pub fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir(dst)?;
    for e in fs::read_dir(src)? {
        let e = e?;
        let a = e.path();
        let b = dst.join(e.file_name());
        let m = fs::symlink_metadata(&a)?;
        if m.file_type().is_symlink() {
            symlink(fs::read_link(&a)?, &b)?;
        } else if m.is_dir() {
            copy_tree(&a, &b)?;
        } else if m.is_file() {
            copy_regular(&a, &b)?;
        } else if !m.file_type().is_socket() {
            bail!("Unsupported special file")
        };
        if !m.file_type().is_symlink() && !m.file_type().is_socket() {
            fs::set_permissions(b, fs::Permissions::from_mode(m.mode() & 0o777))?;
        }
    }
    fs::set_permissions(dst, fs::metadata(src)?.permissions())?;
    Ok(())
}

pub fn fingerprint(root: &Path, p: &Value, cache: &Path, temp: &Path) -> Result<Entries> {
    ensure!(root.is_dir() && !root.is_symlink(), "Missing source folder");
    let previous = optional_json(cache).unwrap_or(Value::Null);
    let mut updated = serde_json::Map::new();
    let mut dbs = BTreeSet::new();
    let mut entries = scan_using(root, p, &mut |path, m| {
        let name = path
            .strip_prefix(root)?
            .to_str()
            .context("Invalid filename")?;
        if is_sqlite(path)? {
            dbs.insert(name.to_owned());
            let snapshot = temp.join(format!("{}.sqlite", digest(&json!(name))));
            let options = p.get("sqlite_recovery").and_then(|v| v.get(name));
            if options.is_some() {
                ensure!(
                    ["device", "desktop", "master-backup"].contains(&mode(p))
                        && encrypted(p)
                        && strings(&p["sqlite"])?.contains(&name.into()),
                    "SQLite recovery requires encrypted backup-only mode"
                );
            }
            sqlite_copy(path, &snapshot, options)?;
            return Ok(json!({"size":fs::metadata(&snapshot)?.len(),"sha256":hash(&snapshot)?}));
        }
        let sig = json!(signature(m));
        let prior = &previous["files"][name];
        let h = if prior["stat"] == sig
            && prior["sha256"]
                .as_str()
                .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            prior["sha256"].as_str().unwrap().to_owned()
        } else {
            hash(path)?
        };
        updated.insert(name.into(), json!({"stat":sig,"sha256":h}));
        Ok(json!({"size":m.len(),"sha256":h}))
    })?;
    if p["telegram_themes"] == true {
        use sha2::{Digest, Sha256};
        for (n, data) in crate::telegram::recovery(root)?.1 {
            entries.get_mut(&n).context("Missing theme")?["size"] = json!(data.len());
            entries.get_mut(&n).unwrap()["sha256"] = json!(format!("{:x}", Sha256::digest(data)));
        }
    }
    for n in p
        .get("sqlite")
        .map(strings)
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .chain(
            p.get("sqlite_recovery")
                .and_then(|v| v.as_object())
                .into_iter()
                .flat_map(|v| v.keys().cloned()),
        )
    {
        ensure!(dbs.contains(&n), "Required SQLite database missing: {n}");
    }
    for n in p
        .get("include")
        .map(strings)
        .transpose()?
        .unwrap_or_default()
    {
        ensure!(entries.contains_key(&n), "Required file missing: {n}");
    }
    atomic_json(cache, &json!({"version":1,"files":updated}))?;
    Ok(entries)
}
