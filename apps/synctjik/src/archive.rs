use crate::{
    fs::{check_tree, scan},
    merge::Entries,
    util::*,
};
use anyhow::{ensure, Context, Result};
use serde_json::json;
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::{symlink, OpenOptionsExt, PermissionsExt},
    path::Path,
};
pub fn pack(tree: &Path, entries: &Entries, path: &Path) -> Result<()> {
    check_tree(entries)?;
    let enc = zstd::Encoder::new(File::create(path)?, 3)?;
    let mut tar = tar::Builder::new(enc);
    for (name, e) in entries {
        let mut h = tar::Header::new_gnu();
        h.set_mode(e["mode"].as_u64().unwrap() as u32);
        h.set_uid(0);
        h.set_gid(0);
        h.set_mtime(0);
        match text(e, "type")? {
            "file" => {
                h.set_entry_type(tar::EntryType::Regular);
                h.set_size(e["size"].as_u64().unwrap());
                tar.append_data(&mut h, name, read_file(&tree.join(name))?)?;
            }
            "dir" => {
                h.set_entry_type(tar::EntryType::Directory);
                h.set_size(0);
                tar.append_data(&mut h, name, io::empty())?;
            }
            _ => {
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_size(0);
                tar.append_link(&mut h, name, text(e, "target")?)?;
            }
        }
    }
    tar.into_inner()?.finish()?.sync_all()?;
    Ok(())
}
pub fn unpack(path: &Path, entries: &Entries, dest: &Path) -> Result<()> {
    check_tree(entries)?;
    ensure!(
        !dest.exists() && !dest.is_symlink(),
        "Extraction target exists"
    );
    fs::create_dir(dest)?;
    fs::set_permissions(dest, fs::Permissions::from_mode(0o700))?;
    // Create only manifest directories first. A link can never be a parent.
    for (n, e) in entries {
        if e["type"] == "dir" {
            fs::create_dir(dest.join(n))?;
        }
    }
    let decoder = zstd::Decoder::new(read_file(path)?)?;
    let mut tar = tar::Archive::new(decoder);
    let mut seen = BTreeSet::new();
    for item in tar.entries()? {
        let mut m = item?;
        let name = m
            .path()?
            .to_str()
            .context("Non-UTF8 archive name")?
            .trim_end_matches('/')
            .to_owned();
        safe_name(&name)?;
        ensure!(seen.insert(name.clone()), "Duplicate archive member");
        let e = entries.get(&name).context("Unexpected archive member")?;
        let t = m.header().entry_type();
        let kind = if t.is_dir() {
            "dir"
        } else if t.is_file() {
            "file"
        } else if t.is_symlink() {
            "link"
        } else {
            "invalid"
        };
        ensure!(e["type"] == kind, "Archive type differs from manifest");
        let p = dest.join(&name);
        match kind {
            "file" => {
                ensure!(
                    m.size() == e["size"].as_u64().context("Invalid file size")?,
                    "Archive size differs from manifest"
                );
                let mut out = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&p)?;
                io::copy(&mut m, &mut out)?;
                fs::set_permissions(
                    p,
                    fs::Permissions::from_mode(e["mode"].as_u64().unwrap() as u32),
                )?;
            }
            "link" => {
                let target = m.link_name()?.context("Missing link target")?;
                ensure!(
                    target.to_str() == Some(text(e, "target")?),
                    "Link differs from manifest"
                );
                symlink(target, &p)?;
            }
            _ => (),
        }
    }
    ensure!(seen.len() == entries.len(), "Missing archive members");
    for (n, e) in entries.iter().rev() {
        if e["type"] == "dir" {
            fs::set_permissions(
                dest.join(n),
                fs::Permissions::from_mode(e["mode"].as_u64().unwrap() as u32),
            )?;
        }
    }
    ensure!(
        scan(dest, &json!({}))? == *entries,
        "Restored files fail manifest verification"
    );
    Ok(())
}
