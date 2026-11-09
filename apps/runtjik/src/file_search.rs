use crate::{discovery::Entry, util::*};
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn row(
    provider: &str,
    id: String,
    name: String,
    subtitle: String,
    icon: &str,
    data: serde_json::Value,
) -> Entry {
    Entry {
        id,
        name,
        subtitle,
        icon: icon.into(),
        provider: provider.into(),
        keywords: String::new(),
        data,
    }
}

pub fn display_root(root: &Path) -> String {
    root.strip_prefix(home())
        .map(|tail| {
            if tail.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", tail.display())
            }
        })
        .unwrap_or_else(|_| root.display().to_string())
}

pub fn open(path: &Path) -> Result<()> {
    let gio = which("gio").context("gio is required to open files")?;
    Command::new(gio)
        .args(["open"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

#[derive(Default)]
pub struct FileSearch {
    root: Option<PathBuf>,
    paths: Vec<String>,
    refreshed: Option<Instant>,
}

impl FileSearch {
    pub fn files(
        &mut self,
        query: &str,
        root: &Path,
        ranking: &serde_json::Value,
        demo: bool,
    ) -> Result<Vec<Entry>> {
        let term = query
            .trim()
            .strip_prefix("/file")
            .or_else(|| query.trim().strip_prefix("/find"))
            .unwrap_or(query)
            .trim();
        if demo {
            return Ok(if term.is_empty() {
                vec![row(
                    "File",
                    "File:hint".into(),
                    "Type to search files".into(),
                    "Demo mode uses synthetic results".into(),
                    "text-x-generic",
                    json!({"disabled":true}),
                )]
            } else {
                vec![row(
                    "File",
                    "File:demo".into(),
                    "demo-notes.md".into(),
                    "~/Demo/demo-notes.md".into(),
                    "text-x-generic",
                    json!({"path":"/demo/demo-notes.md"}),
                )]
            });
        }
        if term.is_empty() {
            return Ok(vec![row(
                "File",
                "File:hint".into(),
                "Type to search files".into(),
                "Search runs below the root shown above".into(),
                "text-x-generic",
                json!({"disabled":true}),
            )]);
        }
        anyhow::ensure!(root.is_dir(), "Search root is not a directory");
        let fd = which("fd").context("Install fd to search files")?;
        let fzf = which("fzf").context("Install fzf to rank file matches")?;
        if self.root.as_deref() != Some(root)
            || self
                .refreshed
                .map_or(true, |at| at.elapsed() > Duration::from_secs(30))
        {
            let found = Command::new(fd)
                .args([
                    "--type",
                    "f",
                    "--hidden",
                    "--color",
                    "never",
                    "--print0",
                    "--max-results",
                    "25000",
                    "--exclude",
                    ".git",
                    "--exclude",
                    "node_modules",
                    "--exclude",
                    ".cache",
                    ".",
                ])
                .arg(root)
                .output()
                .context("Could not enumerate files with fd")?;
            if !found.status.success() {
                bail!("fd could not search {}", display_root(root));
            }
            self.paths = found
                .stdout
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
                .map(|path| String::from_utf8_lossy(path).into_owned())
                .collect();
            self.root = Some(root.to_path_buf());
            self.refreshed = Some(Instant::now());
        }
        let mut input = Vec::new();
        for path in &self.paths {
            input.extend_from_slice(path.as_bytes());
            input.push(0);
        }
        let ranked = Command::new(fzf)
            .args(["--read0", "--print0", "--filter", term])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("Could not start fzf")?;
        let mut ranked = ranked;
        use std::io::Write;
        ranked
            .stdin
            .take()
            .context("fzf stdin unavailable")?
            .write_all(&input)?;
        let ranked = ranked.wait_with_output()?;
        let paths: Vec<String> = ranked
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(|path| String::from_utf8_lossy(path).into_owned())
            .collect();
        let preferred: Vec<String> = strings(&ranking["preferred_extensions"])
            .into_iter()
            .map(|extension| extension.to_lowercase())
            .collect();
        let deprioritized: HashSet<String> = strings(&ranking["deprioritized_extensions"])
            .into_iter()
            .map(|extension| extension.to_lowercase())
            .collect();
        let rank = |path: &str| {
            let extension = Path::new(path)
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            if let Some(position) = preferred.iter().position(|item| item == &extension) {
                (0_u8, position)
            } else if deprioritized.contains(&extension) {
                (2_u8, 0)
            } else {
                (1_u8, 0)
            }
        };
        let mut paths: Vec<_> = paths.into_iter().enumerate().collect();
        paths.sort_by_key(|(fuzzy_rank, path)| (rank(path), *fuzzy_rank));
        let mut rows = Vec::new();
        for (_, path) in paths.into_iter().take(150) {
            let name = Path::new(&path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            rows.push(row(
                "File",
                format!("File:{path}"),
                name,
                path.clone(),
                "text-x-generic",
                json!({"path":path}),
            ));
        }
        if rows.is_empty() {
            rows.push(row(
                "File",
                "File:none".into(),
                "No matching files".into(),
                format!("Search under {}", display_root(root)),
                "text-x-generic",
                json!({"disabled":true}),
            ));
        }
        Ok(rows)
    }
}

pub fn directories(query: &str, demo: bool) -> Result<Vec<Entry>> {
    let term = query.trim().strip_prefix("/dir").unwrap_or(query).trim();
    if demo {
        return Ok(vec![row(
            "Directory",
            "Directory:demo".into(),
            "Demo".into(),
            "~/Demo · synthetic zoxide result".into(),
            "folder",
            json!({"path":"/demo"}),
        )]);
    }
    let zoxide = which("zoxide").context("Install zoxide and initialize it in Fish to use /dir")?;
    let mut command = Command::new(zoxide);
    command.args(["query", "--list"]);
    for word in term.split_whitespace() {
        command.arg(word);
    }
    let output = command.output().context("Could not query zoxide")?;
    if !output.status.success() {
        return Ok(vec![row(
            "Directory",
            "Directory:none".into(),
            "No matching directories".into(),
            "zoxide has no matching visited directory".into(),
            "folder",
            json!({"disabled":true}),
        )]);
    }
    let mut rows = Vec::new();
    for path in String::from_utf8_lossy(&output.stdout).lines().take(150) {
        let path = Path::new(path);
        if !path.is_dir() {
            continue;
        }
        let text = path.to_string_lossy().into_owned();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        rows.push(row(
            "Directory",
            format!("Directory:{text}"),
            name,
            text.clone(),
            "folder",
            json!({"path":text}),
        ));
    }
    if rows.is_empty() {
        rows.push(row(
            "Directory",
            "Directory:none".into(),
            "No matching directories".into(),
            "Try a directory visited from Fish or open a path in Fish to add it to zoxide".into(),
            "folder",
            json!({"disabled":true}),
        ));
    }
    Ok(rows)
}
