use crate::{
    discovery::{folded, Entry},
    util::*,
};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
    thread::JoinHandle,
};

/// Copy text without placing it in argv or a log. Secret clipboard owners exit
/// after one paste so a password is not left available indefinitely.
pub fn copy_text(text: &str, secret: bool) -> Result<()> {
    if let Some(wl_copy) = which("wl-copy") {
        if !secret {
            binary(
                &[wl_copy.to_string_lossy().into_owned()],
                text.as_bytes().to_vec(),
                5,
                true,
            )?;
            return Ok(());
        }
        let mut child = Command::new(wl_copy)
            .arg("--paste-once")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        child
            .stdin
            .take()
            .context("Clipboard input unavailable")?
            .write_all(text.as_bytes())?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        return Ok(());
    }
    if let Some(xclip) = which("xclip") {
        let mut child = Command::new(xclip)
            .args(["-selection", "clipboard"])
            .args(if secret { vec!["-loops", "1"] } else { vec![] })
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        child
            .stdin
            .take()
            .context("Clipboard input unavailable")?
            .write_all(text.as_bytes())?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        return Ok(());
    }
    if secret {
        anyhow::bail!("Secure one-paste clipboard clearing is unavailable")
    }
    binary(
        &["xsel".into(), "--clipboard".into(), "--input".into()],
        text.as_bytes().to_vec(),
        5,
        true,
    )?;
    Ok(())
}

#[derive(Default)]
pub struct Clipboard {
    fingerprints: crate::content_identity::Fingerprints,
    directory: Option<Arc<tempfile::TempDir>>,
    previews: HashMap<String, String>,
    workers: HashMap<String, JoinHandle<String>>,
    pub pending: bool,
}
fn row(id: &str, name: &str, subtitle: &str, data: Value) -> Entry {
    let mut data = data;
    data["keep_open"] = json!(true);
    Entry {
        id: format!("Clipboard:{id}"),
        name: name.into(),
        subtitle: subtitle.into(),
        provider: "Clipboard".into(),
        icon: "edit-paste".into(),
        keywords: String::new(),
        data,
    }
}
impl Clipboard {
    fn preview(&mut self, id: &str, demo: bool) -> Result<String> {
        if let Some(value) = self.previews.get(id) {
            return Ok(value.clone());
        }
        if let Some(handle) = self.workers.get(id) {
            if handle.is_finished() {
                let value = self.workers.remove(id).unwrap().join().unwrap_or_default();
                self.previews.insert(id.into(), value.clone());
                return Ok(value);
            }
        } else if self.workers.len() < 2 {
            let dir = match &self.directory {
                Some(d) => d.clone(),
                None => {
                    let d = Arc::new(
                        tempfile::Builder::new()
                            .prefix("runtjik-clipboard-")
                            .tempdir()?,
                    );
                    self.directory = Some(d.clone());
                    d
                }
            };
            let ident = id.to_owned();
            let handle = std::thread::spawn(move || {
                let path = dir
                    .path()
                    .join(format!("{ident}.{}", if demo { "svg" } else { "png" }));
                let work = || -> Result<()> {
                    if demo {
                        atomic(&path, b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"180\"><rect width=\"320\" height=\"180\" fill=\"#41365c\"/><circle cx=\"160\" cy=\"90\" r=\"60\" fill=\"#fb92ba\"/></svg>")?;
                    } else {
                        let data = binary(
                            &["cliphist".into(), "decode".into()],
                            ident.into_bytes(),
                            5,
                            false,
                        )?;
                        let mut args: Vec<String> = [
                            "magick",
                            "-limit",
                            "memory",
                            "64MiB",
                            "-limit",
                            "map",
                            "128MiB",
                            "-[0]",
                            "-thumbnail",
                            "320x180>",
                        ]
                        .into_iter()
                        .map(str::to_owned)
                        .collect();
                        args.push(path.to_string_lossy().into_owned());
                        binary(&args, data, 10, true)?;
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
                    }
                    Ok(())
                };
                if work().is_ok() {
                    path.to_string_lossy().into_owned()
                } else {
                    let _ = fs::remove_file(path);
                    String::new()
                }
            });
            self.workers.insert(id.into(), handle);
        }
        self.pending = true;
        Ok(String::new())
    }
    pub fn query(&mut self, query: &str, settings: &Value, demo: bool) -> Result<Vec<Entry>> {
        self.pending = false;
        // Reap jobs even when their old result is no longer in this query.
        let ready: Vec<_> = self
            .workers
            .iter()
            .filter(|(_, h)| h.is_finished())
            .map(|(id, _)| id.clone())
            .collect();
        for id in ready {
            let value = self.workers.remove(&id).unwrap().join().unwrap_or_default();
            self.previews.insert(id, value);
        }
        let argument = folded(query.split_once(' ').map(|(_, t)| t).unwrap_or("").trim());
        let (filter, rest) = argument
            .split_once(char::is_whitespace)
            .unwrap_or((&argument, ""));
        let images_only = filter == "images";
        let term = if images_only { rest.trim() } else { &argument };
        let list = if demo {
            "1\tSynthetic clipboard note\n2\thttps://example.com\n3\t[[ binary data 1200 bytes png 32x32 ]]".into()
        } else {
            command(&["cliphist", "list"])?
        };
        let image = regex::Regex::new(r"(?i)\b(png|jpeg|jpg|gif|webp|bmp|tiff)\b")?;
        let size = regex::Regex::new(r"\b\d+x\d+\b")?;
        let mut rows = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for line in list.lines() {
            let Some((id, preview)) = line.split_once('\t') else {
                continue;
            };
            if id.is_empty()
                || !id.bytes().all(|b| b.is_ascii_digit())
                || !folded(preview).contains(term)
            {
                continue;
            }
            let mut name = preview.to_owned();
            let mut thumbnail = String::new();
            let image_kind = preview
                .starts_with("[[ binary data ")
                .then(|| image.find(preview))
                .flatten();
            if images_only && image_kind.is_none() {
                continue;
            }
            let (digest, pending) = if demo {
                (None, false)
            } else {
                self.fingerprints.clipboard(id)
            };
            self.pending |= pending;
            if digest
                .as_ref()
                .is_some_and(|value| !seen.insert(value.clone()))
            {
                continue;
            }
            if let Some(kind) = image_kind {
                name = format!(
                    "{} image{}",
                    kind.as_str().to_uppercase(),
                    size.find(preview)
                        .map(|m| format!(" · {}", m.as_str().replace('x', "×")))
                        .unwrap_or_default()
                );
                thumbnail = self.preview(id, demo)?;
            }
            rows.push(row(
                id,
                &name,
                "",
                json!({"clip_id":id,"thumbnail":thumbnail,"content_hash":digest}),
            ));
            if rows.len()
                >= settings["plugin_settings"]["Clipboard"]["max_results"]
                    .as_u64()
                    .unwrap_or(50) as usize
            {
                break;
            }
        }
        if rows.is_empty() {
            rows.push(row(
                "empty",
                if images_only {
                    "No matching clipboard images"
                } else {
                    "No matching clipboard items"
                },
                "History comes from cliphist; its wl-paste watcher must be running",
                json!({}),
            ));
        }
        Ok(rows)
    }
    pub fn activate(entry: &Entry, mode: &str, demo: bool) -> Result<Value> {
        let Some(id) = entry.data["clip_id"].as_str() else {
            return Ok(json!({"kind":"message","message":"No clipboard item selected"}));
        };
        if mode == "close" {
            if demo {
                return Ok(
                    json!({"kind":"message","message":"Demo clipboard deletion; history unchanged"}),
                );
            }
            binary(
                &["cliphist".into(), "delete".into()],
                id.as_bytes().to_vec(),
                5,
                true,
            )?;
            return Ok(json!({"kind":"message","message":"Deleted clipboard item"}));
        }
        if demo {
            return Ok(
                json!({"kind":"message","message":"Demo clipboard item selected; clipboard unchanged"}),
            );
        }
        let data = binary(
            &["cliphist".into(), "decode".into()],
            id.as_bytes().to_vec(),
            5,
            false,
        )?;
        binary(&["wl-copy".into()], data, 5, true)?;
        Ok(json!({"kind":"activated"}))
    }
}
