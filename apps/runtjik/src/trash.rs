use crate::util::*;
use anyhow::{bail, ensure, Result};
use gio::prelude::*;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::Path,
};
fn items() -> Result<HashMap<String, String>> {
    let root = gio::File::for_uri("trash:///");
    let entries = root.enumerate_children(
        "standard::name,trash::orig-path",
        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
        gio::Cancellable::NONE,
    )?;
    let mut result = HashMap::new();
    while let Some(info) = entries.next_file(gio::Cancellable::NONE)? {
        if let Some(original) = info.attribute_byte_string("trash::orig-path") {
            result.insert(
                root.child(info.name()).uri().to_string(),
                original.to_string(),
            );
        }
    }
    if let (_, Some(error)) = entries.close(gio::Cancellable::NONE) {
        return Err(error.into());
    }
    Ok(result)
}
pub fn action(path: Option<&Path>) -> Result<Value> {
    let state = xdg("state").join("trash-undo.json");
    private_dir(state.parent().unwrap())?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(state.with_extension("lock"))?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } == 0,
        "Cannot lock trash history"
    );
    let mut stack: Vec<Value> = if state.exists() {
        serde_json::from_slice(&fs::read(&state)?)?
    } else {
        vec![]
    };
    let message = if let Some(target) = path {
        ensure!(
            target.is_absolute() && target.is_file() && !target.is_symlink(),
            "Capture is no longer available"
        );
        let before = items()?;
        gio::File::for_path(target).trash(gio::Cancellable::NONE)?;
        let after = items()?;
        let matches: Vec<_> = after
            .iter()
            .filter(|(uri, original)| !before.contains_key(*uri) && Path::new(original) == target)
            .collect();
        ensure!(matches.len()==1,"File is in the desktop Trash, but its undo entry could not be identified; restore it with the file manager");
        stack.push(json!({"uri":matches[0].0,"path":target.to_string_lossy()}));
        format!(
            "Moved to Trash: {} · Ctrl+Z to undo",
            target.file_name().unwrap().to_string_lossy()
        )
    } else {
        let Some(entry) = stack.last() else {
            return Ok(json!({"message":"No capture trash action to undo"}));
        };
        let target = Path::new(s(&entry["path"]));
        let uri = s(&entry["uri"]);
        ensure!(
            !target.exists() && !target.is_symlink(),
            "Cannot restore: a file already exists at {}",
            target.display()
        );
        ensure!(
            items()?.get(uri).map(String::as_str) == Some(s(&entry["path"])),
            "That item is no longer in Trash"
        );
        if !target.is_absolute() {
            bail!("Invalid trash path");
        }
        fs::create_dir_all(target.parent().unwrap())?;
        gio::File::for_uri(uri).move_(
            &gio::File::for_path(target),
            gio::FileCopyFlags::NONE,
            gio::Cancellable::NONE,
            None,
        )?;
        let message = format!(
            "Restored: {}",
            target.file_name().unwrap().to_string_lossy()
        );
        stack.pop();
        message
    };
    atomic(&state, &serde_json::to_vec(&stack)?)?;
    Ok(json!({"message":message}))
}
