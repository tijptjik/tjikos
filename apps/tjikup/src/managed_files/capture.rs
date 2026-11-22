//! Explicit source capture. Never infer template control flow from rendered data.
use super::{
    disk,
    model::{hash, Desired, Snapshot},
    private_dir,
};
use anyhow::{bail, ensure, Context, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    process::Command,
};

pub(super) enum Outcome {
    Unchanged,
    ConfirmLiteral,
    Saved(Vec<Desired>),
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn markers(bytes: &[u8]) -> bool {
    bytes.split(|b| *b == b'\n').any(|line| {
        [b"<<<<<<<".as_slice(), b"=======", b">>>>>>>", b"|||||||"]
            .iter()
            .any(|prefix| line.starts_with(prefix))
    })
}

// Managed source is ours, its rendered content is the common base, local is theirs.
// This retains expressions on untouched lines, without trying to reverse their values.
fn merge(source: &[u8], rendered: &[u8], local: &[u8], literal: bool) -> Result<(Vec<u8>, bool)> {
    let dir = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    for (name, bytes) in [("source", source), ("rendered", rendered), ("local", local)] {
        fs::write(dir.path().join(name), bytes)?;
    }
    let mut cmd = Command::new("git");
    cmd.args([
        "merge-file",
        "-p",
        "--diff3",
        "-L",
        "managed source",
        "-L",
        "rendered managed",
        "-L",
        "local",
    ]);
    if literal {
        cmd.arg("--theirs");
    }
    let result = cmd
        .arg(dir.path().join("source"))
        .arg(dir.path().join("rendered"))
        .arg(dir.path().join("local"))
        .output()?;
    let code = result.status.code().context("source merge interrupted")?;
    ensure!((0..=127).contains(&code), "cannot merge source; use Edit");
    Ok((result.stdout, code != 0))
}

fn replace(
    path: &Path,
    bytes: &[u8],
    previous: &disk::Existing,
    metadata: &disk::Existing,
) -> Result<()> {
    let Snapshot::File { mode, uid, gid, .. } = &metadata.snapshot else {
        bail!("source is not a regular file")
    };
    let desired = Desired {
        path: path.to_owned(),
        bytes: bytes.to_vec(),
        source: None,
        create_only: false,
        sensitive: true,
        snapshot: Snapshot::File {
            sha256: hash(bytes),
            mode: *mode,
            uid: *uid,
            gid: *gid,
        },
    };
    disk::parent(Path::new("/"), path, false)?
        .context("source parent missing")?
        .write(
            &desired,
            &disk::Existing {
                snapshot: previous.snapshot.clone(),
                bytes: Vec::new(),
                xattrs: metadata.xattrs.clone(),
            },
            true,
        )
}

fn reconciliation(source: &[u8], local: &[u8]) -> Vec<u8> {
    let mut out = b"<<<<<<< managed source (retain template logic)\n".to_vec();
    out.extend_from_slice(source);
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    out.extend_from_slice(b"=======\n");
    out.extend_from_slice(local);
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    out.extend_from_slice(b">>>>>>> local (reconcile into the managed source)\n");
    out
}

pub(super) struct Request<'a> {
    pub root: &'a Path,
    pub state_dir: &'a Path,
    pub entry: &'a Desired,
    pub local: &'a disk::Existing,
    pub edit: bool,
    pub literal_confirmed: bool,
}

pub(super) fn update(
    request: Request<'_>,
    tty: &mut File,
    reload: &impl Fn() -> Result<Vec<Desired>>,
) -> Result<Outcome> {
    let Request {
        root,
        state_dir,
        entry,
        local,
        edit,
        literal_confirmed,
    } = request;
    let Some(source) = &entry.source else {
        writeln!(
            tty,
            "Add/Edit requires a regular file with a managed source."
        )?;
        return Ok(Outcome::Unchanged);
    };
    if !matches!(local.snapshot, Snapshot::File { .. }) {
        writeln!(tty, "Add/Edit requires an existing local regular file.")?;
        return Ok(Outcome::Unchanged);
    }
    ensure!(
        source.path != root.join(entry.path.strip_prefix("/")?),
        "source is also the deployed file"
    );
    let before = disk::read(Path::new("/"), &source.path)?;
    ensure!(
        matches!(before.snapshot, Snapshot::File { .. }) && before.bytes == source.bytes,
        "managed source changed during review; rerun apply"
    );
    ensure!(
        disk::read(root, &entry.path)?.snapshot == local.snapshot,
        "local file changed during review; rerun apply"
    );
    if !edit && (source.conditional || entry.sensitive) {
        writeln!(
            tty,
            "{} Use E to edit the managed source.",
            if entry.sensitive {
                "Sensitive content cannot be added from rendered local values."
            } else {
                "Conditional/device-specific source requires manual reconciliation."
            }
        )?;
        return Ok(Outcome::Unchanged);
    }
    let candidate = if edit {
        if entry.sensitive {
            writeln!(tty, "Sensitive source: opening the template only; rendered secrets will not be written into the catalog.")?;
            source.bytes.clone()
        } else {
            if std::str::from_utf8(&source.bytes).is_err()
                || std::str::from_utf8(&local.bytes).is_err()
            {
                writeln!(
                    tty,
                    "Binary source: use Add for plain files; text reconciliation is unavailable."
                )?;
                return Ok(Outcome::Unchanged);
            }
            reconciliation(&source.bytes, &local.bytes)
        }
    } else if source.template {
        let (merged, conflicts) = merge(&source.bytes, &entry.bytes, &local.bytes, false)?;
        if conflicts {
            if !literal_confirmed {
                writeln!(tty, "Warning: changed lines overlap template expressions. Add will replace those conflicting template lines with literal local values, which may affect other devices. Press A again to accept, E to reconcile, or Q to cancel.")?;
                return Ok(Outcome::ConfirmLiteral);
            }
            merge(&source.bytes, &entry.bytes, &local.bytes, true)?.0
        } else {
            merged
        }
    } else {
        local.bytes.clone()
    };
    if !edit && source.template && markers(&candidate) {
        writeln!(tty, "Unresolved merge markers; use E to reconcile.")?;
        return Ok(Outcome::Unchanged);
    }
    // Persist recovery data before modifying a catalog source. The state directory
    // is private and outside the repository, including for sensitive templates.
    let backups = state_dir.join("source-reviews");
    private_dir(&backups, true)?;
    let recovery = tempfile::Builder::new()
        .prefix("review-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(&backups)?
        .keep();
    write_private(&recovery.join("original"), &before.bytes)?;
    write_private(
        &recovery.join("source.json"),
        &serde_json::to_vec_pretty(&serde_json::json!({
            "path": source.path, "snapshot": before.snapshot, "xattrs": before.xattrs,
        }))?,
    )?;
    File::open(&recovery)?.sync_all()?;
    writeln!(
        tty,
        "Managed source: {:?}\nPrivate recovery: {:?}",
        source.path, recovery
    )?;
    replace(&source.path, &candidate, &before, &before)?;
    let result = (|| -> Result<Vec<Desired>> {
        if edit {
            writeln!(tty, "Reconcile the source, remove all conflict markers, then save and close the editor. Source saves remain if deployment is cancelled.")?;
            let editor = std::env::var("VISUAL")
                .or_else(|_| std::env::var("EDITOR"))
                .unwrap_or_else(|_| "zed --wait".into());
            ensure!(!editor.trim().is_empty(), "editor is empty");
            let status = Command::new("sh")
                .args(["-c", &format!("exec {editor} \"$1\""), "tjikup-editor"])
                .arg(&source.path)
                .stdin(tty.try_clone()?)
                .stdout(tty.try_clone()?)
                .stderr(tty.try_clone()?)
                .status()
                .context("open managed source editor")?;
            ensure!(status.success(), "source editor failed");
        }
        let edited = disk::read(Path::new("/"), &source.path)?;
        ensure!(
            matches!(edited.snapshot, Snapshot::File { .. }),
            "edited source is not a regular file"
        );
        ensure!(
            !edit || !markers(&edited.bytes),
            "unresolved conflict markers remain"
        );
        ensure!(
            !edit || edited.bytes != source.bytes,
            "source unchanged; no changes accepted"
        );
        ensure!(
            disk::read(root, &entry.path)?.snapshot == local.snapshot,
            "local file changed during editing"
        );
        let refreshed =
            reload().map_err(|_| anyhow::anyhow!("source validation/rendering failed"))?;
        // All source content is rendered again before deployment; a GUI editor must
        // wait for close. Also reject concurrent catalog edits during validation.
        ensure!(
            disk::read(Path::new("/"), &source.path)?.snapshot == edited.snapshot,
            "source changed during validation"
        );
        // Editors may replace the inode or alter metadata; retain original policy.
        replace(&source.path, &edited.bytes, &edited, &before)?;
        Ok(refreshed)
    })();
    match result {
        Ok(refreshed) => {
            writeln!(
                tty,
                "Saved managed source; deployment will use the newly rendered content."
            )?;
            Ok(Outcome::Saved(refreshed))
        }
        Err(error) => {
            let edited = disk::read(Path::new("/"), &source.path)?;
            write_private(&recovery.join("unfinished"), &edited.bytes)?;
            replace(&source.path, &before.bytes, &edited, &before)?;
            writeln!(
                tty,
                "{error}; original source restored. Unfinished edits are in {:?}.",
                recovery
            )?;
            Ok(Outcome::Unchanged)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merge_preserves_untouched_expression() {
        let (bytes, conflict) = merge(
            b"host={{ host }}\n\none\n",
            b"host=fi\n\none\n",
            b"host=fi\n\ntwo\n",
            false,
        )
        .unwrap();
        assert!(!conflict);
        assert_eq!(bytes, b"host={{ host }}\n\ntwo\n");
    }
    #[test]
    fn expression_change_requires_literal_confirmation() {
        let source = b"host={{ host }}\n";
        let rendered = b"host=fi\n";
        let local = b"host=li\n";
        assert!(merge(source, rendered, local, false).unwrap().1);
        assert_eq!(merge(source, rendered, local, true).unwrap().0, local);
    }
}
