//! Native file deployment, deliberately independent of the recipe scheduler.
mod disk;
mod model;
mod secrets;

use anyhow::{bail, Context, Result};
use model::{Desired, Manifest, Snapshot};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    path: PathBuf,
    before: Snapshot,
    after: Snapshot,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u32,
    id: String,
    root: PathBuf,
    files: BTreeMap<PathBuf, Snapshot>,
    pending: Option<Pending>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    path: PathBuf,
    before: Snapshot,
    after: Snapshot,
    backup: Option<String>,
    xattrs: BTreeMap<String, Vec<u8>>,
    status: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    id: String,
    root: PathBuf,
    changes: Vec<Change>,
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut temp = tempfile::Builder::new()
        .prefix(".state-")
        .tempfile_in(path.parent().unwrap())?;
    serde_json::to_writer_pretty(&mut temp, value)?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}

fn private_dir(path: &Path, create: bool) -> Result<()> {
    model::absolute(path)?;
    let mut current = PathBuf::from("/");
    for part in path.strip_prefix("/")?.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => bail!(
                "state path contains a symlink or non-directory: {}",
                current.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if !create {
                    return Ok(());
                }
                fs::DirBuilder::new().mode(0o700).create(&current)?;
            }
            Err(e) => return Err(e.into()),
        }
    }
    let meta = fs::metadata(path)?;
    if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        bail!(
            "state directory must be owned by the invoking user with mode 0700: {}",
            path.display()
        );
    }
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        bail!("state/receipt must be a private regular file owned by the invoking user");
    }
    Ok(serde_json::from_reader(file)?)
}

fn load_state(path: &Path, id: &str, root: &Path) -> Result<(State, bool)> {
    let mut state = if path.try_exists()? {
        read_json::<State>(path)?
    } else {
        State {
            version: 1,
            id: id.into(),
            root: root.into(),
            files: BTreeMap::new(),
            pending: None,
        }
    };
    if state.version != 1 || state.id != id || state.root != root {
        bail!("state belongs to a different manifest or destination root");
    }
    let recovered = state.pending.is_some();
    if let Some(pending) = state.pending.take() {
        model::absolute(&pending.path)?;
        let current = disk::read(root, &pending.path)?.snapshot;
        if current == pending.after {
            state.files.insert(pending.path, current);
        } else if current != pending.before {
            bail!(
                "interrupted apply has conflicting changes: {}",
                pending.path.display()
            );
        }
    }
    Ok((state, recovered))
}

fn conflict(desired: &Desired, current: &Snapshot, state: &State) -> bool {
    if current == &desired.snapshot || desired.create_only && current != &Snapshot::Absent {
        return false;
    }
    match state.files.get(&desired.path) {
        Some(previous) => previous != current,
        None => current != &Snapshot::Absent,
    }
}

#[derive(Serialize)]
struct PlanEntry {
    path: PathBuf,
    action: &'static str,
    sensitive: bool,
}

fn plan(root: &Path, entries: &[Desired], state: &State) -> Result<Vec<PlanEntry>> {
    entries
        .iter()
        .map(|entry| {
            let current = disk::read(root, &entry.path)
                .with_context(|| format!("inspect {}", entry.path.display()))?;
            if matches!(current.snapshot, Snapshot::Directory { .. })
                && !matches!(
                    entry.snapshot,
                    Snapshot::Directory { .. } | Snapshot::Absent
                )
                || matches!(entry.snapshot, Snapshot::Directory { .. })
                    && !matches!(
                        current.snapshot,
                        Snapshot::Directory { .. } | Snapshot::Absent
                    )
            {
                bail!(
                    "directory/file replacement is unsupported: {}",
                    entry.path.display()
                );
            }
            let action = if current.snapshot == entry.snapshot
                || entry.create_only && current.snapshot != Snapshot::Absent
            {
                "unchanged"
            } else if conflict(entry, &current.snapshot, state) {
                "conflict"
            } else if entry.snapshot == Snapshot::Absent {
                "remove"
            } else if current.snapshot == Snapshot::Absent {
                "create"
            } else {
                "update"
            };
            Ok(PlanEntry {
                path: entry.path.clone(),
                action,
                sensitive: entry.sensitive,
            })
        })
        .collect()
}

fn apply(
    root: &Path,
    entries: &[Desired],
    state_path: &Path,
    mut state: State,
    restore_xattrs: Option<&BTreeMap<PathBuf, BTreeMap<String, Vec<u8>>>>,
) -> Result<usize> {
    let plans = plan(root, entries, &state)?;
    if let Some(entry) = plans.iter().find(|p| p.action == "conflict") {
        bail!(
            "local changes or unmanaged existing file: {}; no files applied",
            entry.path.display()
        );
    }
    let count = plans.iter().filter(|p| p.action != "unchanged").count();
    let mut receipt = Receipt {
        version: 1,
        id: state.id.clone(),
        root: root.into(),
        changes: Vec::new(),
    };
    let run = if count > 0 {
        Some(
            tempfile::Builder::new()
                .prefix("receipt-")
                .tempdir_in(state_path.parent().unwrap())?
                .keep(),
        )
    } else {
        None
    };
    let mut previous = Vec::new();
    // Complete rendering and conflict checks precede all changes. Back up every
    // replacement before the first write, including metadata and extended attrs.
    for (entry, planned) in entries.iter().zip(&plans) {
        let existing = disk::read(root, &entry.path)?;
        if conflict(entry, &existing.snapshot, &state) {
            bail!(
                "destination changed during preparation: {}",
                entry.path.display()
            );
        }
        if planned.action != "unchanged" {
            let backup = if matches!(existing.snapshot, Snapshot::File { .. }) {
                let name = format!("{}.bin", receipt.changes.len());
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(run.as_ref().unwrap().join(&name))?;
                file.write_all(&existing.bytes)?;
                file.sync_all()?;
                Some(name)
            } else {
                None
            };
            receipt.changes.push(Change {
                path: entry.path.clone(),
                before: existing.snapshot.clone(),
                after: entry.snapshot.clone(),
                backup,
                xattrs: existing.xattrs.clone(),
                status: "prepared".into(),
            });
        }
        previous.push(existing);
    }
    if let Some(run) = &run {
        atomic_json(&run.join("receipt.json"), &receipt)?;
    }
    let mut change_index = 0;
    for ((entry, planned), mut existing) in entries.iter().zip(&plans).zip(previous) {
        if planned.action != "unchanged" {
            let parent =
                disk::parent(root, &entry.path, true)?.context("missing destination parent")?;
            state.pending = Some(Pending {
                path: entry.path.clone(),
                before: existing.snapshot.clone(),
                after: entry.snapshot.clone(),
            });
            atomic_json(state_path, &state)?;
            if let Some(attrs) = restore_xattrs.and_then(|all| all.get(&entry.path)) {
                existing.xattrs = attrs.clone();
            }
            if let Err(error) = parent.write(entry, &existing, root == Path::new("/")) {
                receipt.changes[change_index].status = "failed".into();
                atomic_json(&run.as_ref().unwrap().join("receipt.json"), &receipt)?;
                bail!(
                    "apply failed for {}: {error:#}; recovery receipt: {}",
                    entry.path.display(),
                    run.as_ref().unwrap().join("receipt.json").display()
                );
            }
            receipt.changes[change_index].status = "applied".into();
            change_index += 1;
            atomic_json(&run.as_ref().unwrap().join("receipt.json"), &receipt)?;
            existing.snapshot = entry.snapshot.clone();
        } else if disk::read(root, &entry.path)?.snapshot != existing.snapshot {
            bail!(
                "unchanged destination changed during apply: {}",
                entry.path.display()
            );
        }
        let changed =
            state.files.get(&entry.path) != Some(&existing.snapshot) || state.pending.is_some();
        state.files.insert(entry.path.clone(), existing.snapshot);
        state.pending = None;
        if changed {
            atomic_json(state_path, &state)?;
        }
    }
    if let Some(run) = &run {
        println!("Receipt: {}", run.join("receipt.json").display());
    }
    Ok(count)
}

struct Options {
    command: String,
    manifest: Option<PathBuf>,
    receipt: Option<PathBuf>,
    root: PathBuf,
    state: PathBuf,
    json: bool,
    context: Option<PathBuf>,
}
fn options(args: Vec<String>) -> Result<Options> {
    let mut args = args.into_iter();
    let command = args.next().context("usage: tjikup files <check|plan|apply|restore> --manifest PATH --state-dir PATH [--root PATH] [--json]")?;
    if !["check", "plan", "apply", "restore"].contains(&command.as_str()) {
        bail!("unknown files operation: {command}");
    }
    let (mut manifest, mut receipt, mut state, mut root, mut json) =
        (None, None, None, PathBuf::from("/"), false);
    let mut context = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--manifest" => {
                manifest = Some(PathBuf::from(
                    args.next().context("--manifest needs a path")?,
                ))
            }
            "--receipt" => {
                receipt = Some(PathBuf::from(
                    args.next().context("--receipt needs a path")?,
                ))
            }
            "--state-dir" => {
                state = Some(PathBuf::from(
                    args.next().context("--state-dir needs a path")?,
                ))
            }
            "--root" => root = PathBuf::from(args.next().context("--root needs a path")?),
            "--json" => json = true,
            "--context-file" => {
                context = Some(PathBuf::from(
                    args.next().context("--context-file needs a path")?,
                ))
            }
            _ => bail!("unknown files argument: {arg}"),
        }
    }
    if command == "restore" {
        if receipt.is_none() || manifest.is_some() || context.is_some() {
            bail!("restore requires --receipt and no --manifest");
        }
    } else if manifest.is_none() || receipt.is_some() {
        bail!("files command requires --manifest and no --receipt");
    }
    if !root.is_absolute() {
        bail!("--root must be absolute");
    }
    let root = root
        .canonicalize()
        .context("destination root must already exist")?;
    let state =
        state.context("--state-dir is required; keep deployment state outside the repository")?;
    model::absolute(&state)?;
    Ok(Options {
        command,
        manifest,
        receipt,
        state,
        root,
        json,
        context,
    })
}

fn hostname() -> Result<String> {
    let mut buffer = [0u8; 256];
    if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let len = buffer
        .iter()
        .position(|c| *c == 0)
        .context("hostname too long")?;
    Ok(String::from_utf8(buffer[..len].to_vec())?)
}

fn run(args: Vec<String>) -> Result<i32> {
    let opts = options(args)?;
    let mut restored_attrs = None;
    let mut restore_expected = BTreeMap::new();
    let (id, entries) = if let Some(path) = &opts.manifest {
        let (manifest, entries) = Manifest::load(path, opts.context.as_deref())?;
        (manifest.id, entries)
    } else {
        let receipt_path = opts.receipt.as_ref().unwrap();
        let receipt: Receipt = read_json(receipt_path)?;
        if receipt.version != 1 || receipt.root != opts.root {
            bail!("receipt belongs to another root/version");
        }
        let mut entries = Vec::new();
        let mut attrs = BTreeMap::new();
        for change in receipt.changes.iter().rev() {
            model::absolute(&change.path)?;
            let current = disk::read(&opts.root, &change.path)?.snapshot;
            if current == change.before {
                continue;
            }
            if current != change.after {
                bail!(
                    "cannot restore locally changed destination: {}",
                    change.path.display()
                );
            }
            let bytes = if let Snapshot::File { sha256, .. } = &change.before {
                let name = change.backup.as_ref().context("missing backup")?;
                if Path::new(name).components().count() != 1
                    || name.starts_with('.')
                    || Path::new(name).is_absolute()
                {
                    bail!("invalid backup path");
                }
                let bytes = fs::read(receipt_path.parent().unwrap().join(name))?;
                if model::hash(&bytes) != *sha256 {
                    bail!("backup checksum mismatch");
                }
                bytes
            } else {
                Vec::new()
            };
            restore_expected.insert(change.path.clone(), current);
            attrs.insert(change.path.clone(), change.xattrs.clone());
            entries.push(Desired {
                path: change.path.clone(),
                snapshot: change.before.clone(),
                bytes,
                create_only: false,
                sensitive: true,
            });
        }
        restored_attrs = Some(attrs);
        (receipt.id, entries)
    };
    if id.is_empty()
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        bail!("invalid manifest id");
    }
    // Do not allow a manifest to overwrite its own state/receipts/lock.
    for entry in &entries {
        let destination = opts.root.join(entry.path.strip_prefix("/")?);
        if destination.starts_with(&opts.state)
            || opts.state.starts_with(&destination)
                && !matches!(entry.snapshot, Snapshot::Directory { .. })
        {
            bail!("destination overlaps deployment state");
        }
    }
    let mutate = opts.command == "apply" || opts.command == "restore";
    private_dir(&opts.state, mutate)?;
    let _lock = if mutate {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(opts.state.join("lock"))?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            bail!("another file deployment is running");
        }
        Some(lock)
    } else {
        None
    };
    for (path, expected) in &restore_expected {
        if disk::read(&opts.root, path)?.snapshot != *expected {
            bail!(
                "restore destination changed while acquiring the deployment lock: {}",
                path.display()
            );
        }
    }
    let state_path = opts.state.join(format!("{id}.json"));
    let (state, recovered) = load_state(&state_path, &id, &opts.root)?;
    let plans = plan(&opts.root, &entries, &state)?;
    let conflicts = plans.iter().any(|p| p.action == "conflict");
    let changed = plans.iter().filter(|p| p.action != "unchanged").count();
    if !mutate {
        if opts.json {
            println!("{}", serde_json::to_string_pretty(&plans)?);
        } else {
            for p in &plans {
                println!("{} {}", p.action.to_uppercase(), p.path.display());
            }
        }
        return Ok(if conflicts {
            2
        } else if changed > 0 {
            1
        } else {
            0
        });
    }
    if recovered {
        atomic_json(&state_path, &state)?;
    }
    let changed = apply(
        &opts.root,
        &entries,
        &state_path,
        state,
        restored_attrs.as_ref(),
    )?;
    println!(
        "{}: {changed} changed, {} unchanged",
        if changed == 0 { "SKIP" } else { "APPLY" },
        entries.len() - changed
    );
    Ok(0)
}

pub fn cli(args: Vec<String>) -> i32 {
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        println!("tjikup files <check|plan|apply> --manifest PATH --state-dir PATH [--root PATH] [--json]\ntjikup files restore --receipt PATH --state-dir PATH [--root PATH]\n\ncheck/plan: 0 current, 1 changes needed, 2 conflict/error; no writes.\napply: track desired files, preserve local edits, back up replacements.\nrestore: verify and restore a private receipt; refuse later edits.\n--root remaps absolute destinations into an existing staging directory.\nSee docs/native-files.md for the manifest schema and migration boundary.");
        return 0;
    }
    match run(args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("tjikup files: {error:#}");
            2
        }
    }
}
