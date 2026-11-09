use crate::{discovery::Entry, util::*};
use anyhow::{Context, Result};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
};

pub struct Notes {
    scratch: PathBuf,
    vault: Option<PathBuf>,
    destination: Option<PathBuf>,
    promoting: Option<PathBuf>,
}

impl Default for Notes {
    fn default() -> Self {
        let scratch = std::env::var_os("RUNTJIK_NOTES_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| xdg("state").join("scratchpad"));
        let configured = std::env::var_os("RUNTJIK_OBSIDIAN_VAULT")
            .map(PathBuf::from)
            .or_else(|| {
                let path = xdg("state").join("obsidian-vault.json");
                fs::read(path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<PathBuf>(&bytes).ok())
            });
        Self {
            scratch,
            vault: configured.filter(|path| path.is_dir()),
            destination: None,
            promoting: None,
        }
    }
}

fn row(id: String, name: String, subtitle: String, icon: &str, data: serde_json::Value) -> Entry {
    Entry {
        id,
        name,
        subtitle,
        icon: if icon == "runtjik-note" {
            crate::paths::command_icon("note")
        } else {
            icon.into()
        },
        provider: "Note".into(),
        keywords: String::new(),
        data,
    }
}

fn open(path: &Path) -> Result<()> {
    let viewer = which("obsidian-viewer")
        .context("Install the Obsidian Markdown viewer with the Tjikup Obsidian workflow")?;
    Command::new(viewer)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

fn vault_candidates() -> Vec<PathBuf> {
    let roots = ["Documents", "Notes", "Obsidian", "Vaults", "Sync", "org"]
        .into_iter()
        .map(|name| home().join(name));
    let mut found = Vec::new();
    for root in roots.filter(|root| root.is_dir()) {
        for entry in walkdir::WalkDir::new(root)
            .max_depth(5)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                ![".cache", ".git", "node_modules", "target"].contains(&name.as_ref())
            })
            .filter_map(|entry| entry.ok())
        {
            if entry.file_type().is_dir() && entry.file_name() == ".obsidian" {
                if let Some(vault) = entry.path().parent() {
                    found.push(vault.to_path_buf());
                }
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

impl Notes {
    pub fn set_vault(&mut self, path: &Path) -> Result<()> {
        let path = if path.file_name().is_some_and(|name| name == ".obsidian") {
            path.parent().unwrap_or(path).to_path_buf()
        } else {
            path.to_path_buf()
        };
        anyhow::ensure!(
            path.is_dir() && path.join(".obsidian").is_dir(),
            "Choose an Obsidian vault folder containing .obsidian"
        );
        let record = xdg("state").join("obsidian-vault.json");
        atomic(&record, &serde_json::to_vec(&path)?)?;
        self.vault = Some(path);
        Ok(())
    }

    pub fn query(&mut self, query: &str, demo: bool) -> Result<Vec<Entry>> {
        let rest = query.trim().strip_prefix("/note").unwrap_or("").trim();
        if demo {
            return Ok(vec![row(
                "Note:demo".into(),
                "Demo scratch note".into(),
                "Synthetic preview · no notes are read or written".into(),
                "runtjik-note",
                json!({"path":"/demo/scratch.md","demo":true,"keep_open":true}),
            )]);
        }
        if rest == "vault" || rest.starts_with("vault ") {
            if self.vault.is_none() {
                self.vault = vault_candidates().into_iter().next();
            }
            let Some(vault) = self.vault.clone() else {
                return Ok(vec![row(
                    "Note:no-vault".into(),
                    "Choose an Obsidian vault".into(),
                    "Use the header folder button to choose a folder containing .obsidian".into(),
                    "folder",
                    json!({"disabled":true}),
                )]);
            };
            let here = self.destination.clone().unwrap_or_else(|| vault.clone());
            let Some(note) = self.promoting.clone() else {
                return Ok(vec![row(
                    "Note:no-promotion".into(),
                    "Choose a note to promote first".into(),
                    "Shift+Enter on a scratch note opens this folder tree".into(),
                    "runtjik-note",
                    json!({"disabled":true}),
                )]);
            };
            let tree_term = rest
                .strip_prefix("vault")
                .unwrap_or("")
                .trim()
                .to_lowercase();
            let mut rows = vec![row(
                "Note:folder-here".into(),
                "＋ Use this folder".into(),
                here.display().to_string(),
                "folder-open",
                json!({"path":here,"note":note,"folder_here":true,"keep_open":true}),
            )];
            for entry in fs::read_dir(&here)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir()
                    && !entry.file_name().to_string_lossy().starts_with('.')
                    && (tree_term.is_empty()
                        || entry
                            .file_name()
                            .to_string_lossy()
                            .to_lowercase()
                            .contains(&tree_term))
                {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    rows.push(row(
                        format!("Note:folder:{}", path.display()),
                        name,
                        path.display().to_string(),
                        "folder",
                        json!({"path":path,"note":note,"folder":true,"keep_open":true}),
                    ));
                }
            }
            rows.push(row(
                "Note:new-directory".into(),
                "＋ New Directory".into(),
                "Create a subfolder here".into(),
                "folder-new",
                json!({"path":here,"note":note,"new_directory":true,"keep_open":true}),
            ));
            return Ok(rows);
        }
        fs::create_dir_all(&self.scratch)?;
        let term = if rest == "history" {
            ""
        } else {
            rest.strip_prefix("history ").unwrap_or(rest)
        }
        .trim()
        .to_lowercase();
        let mut notes = Vec::new();
        for entry in walkdir::WalkDir::new(&self.scratch)
            .max_depth(3)
            .follow_links(false)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.file_type().is_file()
                    && entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("md"))
            })
        {
            let path = entry.path();
            let name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let body = if term.is_empty() {
                String::new()
            } else {
                fs::read_to_string(path).unwrap_or_default()
            };
            if !term.is_empty()
                && !name.to_lowercase().contains(&term)
                && !body.to_lowercase().contains(&term)
            {
                continue;
            }
            let modified = entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok());
            notes.push((modified, name, path.to_path_buf()));
        }
        notes.sort_by(|a, b| b.0.cmp(&a.0));
        let mut rows = vec![row(
            "Note:new".into(),
            "NEW".into(),
            "Create and open a scratch note in the Markdown viewer".into(),
            "runtjik-note".into(),
            json!({"new":true,"keep_open":true}),
        )];
        rows.extend(notes.into_iter().take(150).map(|(_, name, path)| {
            row(
                format!("Note:{}", path.display()),
                name,
                path.display().to_string(),
                "runtjik-note",
                json!({"path":path,"note":true,"keep_open":true}),
            )
        }));
        if rows.len() == 1 {
            rows.push(row(
                "Note:empty".into(),
                "No scratch notes yet".into(),
                self.scratch.display().to_string(),
                "runtjik-note",
                json!({"disabled":true}),
            ));
        }
        Ok(rows)
    }

    pub fn begin_promotion(&mut self, path: PathBuf) -> Result<()> {
        if self.vault.is_none() {
            self.vault = vault_candidates().into_iter().next();
        }
        anyhow::ensure!(
            self.vault.is_some(),
            "Choose an Obsidian vault from the header before promoting notes"
        );
        self.destination = self.vault.clone();
        self.promoting = Some(path);
        Ok(())
    }

    pub fn cancel_promotion(&mut self) {
        self.promoting = None;
        self.destination = None;
    }

    pub fn activate(&mut self, entry: &Entry, mode: &str, demo: bool) -> Result<serde_json::Value> {
        if demo {
            return Ok(json!({"kind":"message","message":"Demo mode leaves note files unchanged"}));
        }
        if entry.data["new"] == true {
            let stamp = chrono::Local::now().format("%Y-%m-%d_%H%M%S");
            private_dir(&self.scratch)?;
            let path = self.scratch.join(format!("{stamp}.md"));
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
            open(&path)?;
            return Ok(json!({"kind":"activated","message":"Opened a new scratch note"}));
        }
        let path = PathBuf::from(s(&entry.data["path"]));
        if entry.data["note"] == true && mode == "close" {
            let result = crate::trash::action(Some(&path))?;
            return Ok(json!({"kind":"message","message":result["message"]}));
        }
        if entry.data["note"] == true && mode == "switch" {
            self.begin_promotion(path)?;
            self.destination = self.vault.clone();
            return Ok(json!({"open_query":"/note vault"}));
        }
        if entry.data["new_directory"] == true {
            let parent = path;
            let note = PathBuf::from(s(&entry.data["note"]));
            let mut target = parent.join("New Directory");
            let mut suffix = 2;
            while target.exists() {
                target = parent.join(format!("New Directory {suffix}"));
                suffix += 1;
            }
            fs::create_dir(&target)?;
            self.destination = Some(target);
            self.promoting = Some(note);
            return Ok(json!({"open_query":"/note vault","message":"Created a new directory"}));
        }
        if entry.data["folder"] == true && mode == "launch" {
            self.destination = Some(path);
            return Ok(json!({"open_query":"/note vault"}));
        }
        if entry.data["folder_here"] == true && mode == "switch" {
            let note = PathBuf::from(s(&entry.data["note"]));
            let target = path.join(note.file_name().context("Scratch note has no filename")?);
            let mut target = target;
            let mut suffix = 2;
            while target.exists() {
                let stem = note.file_stem().unwrap_or_default().to_string_lossy();
                target = path.join(format!("{stem} ({suffix}).md"));
                suffix += 1;
            }
            fs::copy(&note, &target)?;
            self.cancel_promotion();
            return Ok(
                json!({"open_query":"/note","message":format!("Promoted note to {}", target.display())}),
            );
        }
        if entry.data["note"] == true && path.is_file() {
            open(&path)?;
            return Ok(json!({"kind":"activated","message":"Opened note"}));
        }
        Ok(json!({"kind":"message","message":"Choose a note or folder"}))
    }
}
