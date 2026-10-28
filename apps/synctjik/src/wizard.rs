use anyhow::{ensure, Context, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute, queue,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, IsTerminal, Read, Write},
    path::Path,
};
use synctjik::{engine::Engine, fs as tree, merge, util::*};
pub fn ask(label: &str, default: &str) -> Result<String> {
    ensure!(io::stdin().is_terminal(), "Interactive terminal required");
    eprint!("{} [{}]: ", tjik_ui::safe(label), tjik_ui::safe(default));
    io::stderr().flush()?;
    let mut s = String::new();
    ensure!(io::stdin().read_line(&mut s)? > 0, "Input closed");
    let s = s.trim();
    Ok(if s.is_empty() {
        default.into()
    } else {
        s.into()
    })
}
pub fn confirm(label: &str) -> Result<bool> {
    Ok(matches!(ask(label, "y/N")?.as_str(), "y" | "Y" | "yes"))
}
pub fn text_diff(left: &Path, le: &Value, right: &Path, re: &Value, file: &str) -> Result<String> {
    safe_name(file)?;
    let mut content = Vec::new();
    for (root, entries) in [(left, le), (right, re)] {
        let e = &entries[file];
        if e.is_null() {
            content.push(String::new());
            continue;
        }
        if e["type"] != "file" || e["size"].as_u64().unwrap_or(u64::MAX) > 1024 * 1024 {
            return Ok(format!("Local: {}\nSource: {}", le[file], re[file]));
        }
        let mut raw = Vec::new();
        read_file(&root.join(file))?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut raw)?;
        ensure!(raw.len() <= 1024 * 1024, "File grew during review");
        if raw.contains(&0) {
            return Ok(format!(
                "Binary file\nLocal: {}\nSource: {}",
                le[file], re[file]
            ));
        }
        content.push(String::from_utf8(raw).context("Non-text file; inspect manifest metadata")?);
    }
    Ok(similar::TextDiff::from_lines(&content[0], &content[1])
        .unified_diff()
        .header("local", "source")
        .to_string())
}
pub fn preview(app: &Engine, plan: &Value, file: &str) -> Result<String> {
    let folder = text(plan, "folder")?;
    let local = tree::safe_root(&app.home, folder)?;
    ensure!(
        json!(tree::scan(&local, &plan["policy"])?) == plan["local"],
        "Local files changed; create a new plan"
    );
    let temp = tempfile::tempdir_in(&app.state)?;
    let (r, m) = app.fetch(
        folder,
        text(plan, "source")?,
        temp.path(),
        Some(text(&plan["ref"], "snapshot")?),
        true,
    )?;
    ensure!(
        r == plan["ref"] && m["entries"] == plan["incoming"],
        "Snapshot changed"
    );
    text_diff(
        &local,
        &plan["local"],
        &temp.path().join("tree"),
        &m["entries"],
        file,
    )
}
struct Screen {
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    signals: Vec<signal_hook::SigId>,
}
impl Screen {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        if let Err(e) = execute!(io::stderr(), EnterAlternateScreen, cursor::Hide) {
            let _ = terminal::disable_raw_mode();
            return Err(e.into());
        }
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut screen = Self {
            cancelled: cancelled.clone(),
            signals: Vec::new(),
        };
        for signal in [
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGHUP,
        ] {
            screen
                .signals
                .push(signal_hook::flag::register(signal, cancelled.clone())?);
        }
        Ok(screen)
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(io::stderr(), cursor::Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
        for id in &self.signals {
            signal_hook::low_level::unregister(*id);
        }
    }
}
fn decisions(
    app: &Engine,
    plan: &Value,
    previous: &BTreeMap<String, String>,
) -> Result<Option<BTreeMap<String, String>>> {
    ensure!(
        io::stdin().is_terminal() && io::stderr().is_terminal(),
        "Interactive review needs a terminal"
    );
    let folder = text(plan, "folder")?;
    let local = tree::safe_root(&app.home, folder)?;
    ensure!(
        json!(tree::scan(&local, &plan["policy"])?) == plan["local"],
        "Local state changed"
    );
    let temp = tempfile::tempdir_in(&app.state)?;
    let (r, m) = app.fetch(
        folder,
        text(plan, "source")?,
        temp.path(),
        Some(text(&plan["ref"], "snapshot")?),
        true,
    )?;
    ensure!(
        r == plan["ref"] && m["entries"] == plan["incoming"],
        "Snapshot changed"
    );
    let names: Vec<_> = plan["local"]
        .as_object()
        .unwrap()
        .keys()
        .chain(plan["incoming"].as_object().unwrap().keys())
        .filter(|n| plan["local"][*n] != plan["incoming"][*n])
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if names.is_empty() {
        return Ok(Some(BTreeMap::new()));
    }
    let mut selected = 0usize;
    let mut scroll = 0usize;
    let mut focus_diff = false;
    let mut choices = previous.clone();
    let mut cache = BTreeMap::new();
    let mut notice = String::new();
    let screen = Screen::enter()?;
    loop {
        let (w, h) = terminal::size()?;
        let rows = (h as usize).saturating_sub(5);
        let left = (w as usize / 3).max(18);
        let right = (w as usize).saturating_sub(left + 3);
        let name = &names[selected];
        let diff = cache.entry(name.clone()).or_insert_with(|| {
            text_diff(
                &local,
                &plan["local"],
                &temp.path().join("tree"),
                &plan["incoming"],
                name,
            )
            .unwrap_or_else(|_| "Cannot display as text; compare manifest metadata.".into())
        });
        let lines: Vec<_> = diff.lines().map(tjik_ui::safe).collect();
        queue!(io::stderr(), cursor::MoveTo(0, 0), Clear(ClearType::All))?;
        if w < 60 || h < 10 {
            eprint!("Resize terminal to at least 60×10; q quits");
        } else {
            eprint!(
                "{}\r\n",
                tjik_ui::fit(
                    &format!(
                        "Synctjik · {folder} ← {} · {}",
                        text(plan, "source")?,
                        if focus_diff { "preview" } else { "files" }
                    ),
                    w as usize
                )
            );
            let offset = selected
                .saturating_sub(rows / 2)
                .min(names.len().saturating_sub(rows));
            for row in 0..rows {
                let i = offset + row;
                let label = if let Some(n) = names.get(i) {
                    format!(
                        "{} [{}] {}",
                        if i == selected { "›" } else { " " },
                        match choices.get(n).map(String::as_str) {
                            Some("source") => "accepted",
                            Some("local") => "rejected",
                            Some("skip") => "skipped",
                            _ => "pending",
                        },
                        n
                    )
                } else {
                    String::new()
                };
                let line = lines.get(scroll + row).map(String::as_str).unwrap_or("");
                eprint!(
                    "{:<left$} │ {}\r\n",
                    tjik_ui::fit(&tjik_ui::safe(&label), left),
                    tjik_ui::fit(line, right)
                );
            }
            eprint!(
                "{}\r\n",
                tjik_ui::fit(
                    "↑↓ select/scroll · Tab focus · a accept · r retain · s skip · u undo · A all",
                    w as usize
                )
            );
            eprint!(
                "{}\r\n",
                tjik_ui::fit(
                    "Enter review complete · S skip folder · q cancel",
                    w as usize
                )
            );
            eprint!("{}", tjik_ui::fit(&notice, w as usize));
        }
        io::stderr().flush()?;
        if screen.cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(None);
        }
        if !event::poll(std::time::Duration::from_millis(150))? {
            continue;
        }
        match event::read()? {
            Event::Key(k) => {
                if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
                    return Ok(None);
                }
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('S') => return Ok(None),
                    KeyCode::Tab => focus_diff = !focus_diff,
                    KeyCode::Up | KeyCode::Char('k') => {
                        if focus_diff {
                            scroll = scroll.saturating_sub(1)
                        } else {
                            selected = selected.saturating_sub(1);
                            scroll = 0
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if focus_diff {
                            scroll = (scroll + 1).min(lines.len().saturating_sub(1))
                        } else {
                            selected = (selected + 1).min(names.len() - 1);
                            scroll = 0
                        }
                    }
                    KeyCode::PageDown => {
                        scroll = (scroll + rows.max(1)).min(lines.len().saturating_sub(1))
                    }
                    KeyCode::PageUp => scroll = scroll.saturating_sub(rows.max(1)),
                    KeyCode::Char('a') => {
                        choices.insert(name.clone(), "source".into());
                    }
                    KeyCode::Char('r') => {
                        if mode(&plan["policy"]) == "master" {
                            notice="Leader mirrors require source contents; skip the folder to keep local state.".into();
                        } else {
                            choices.insert(name.clone(), "local".into());
                        }
                    }
                    KeyCode::Char('s') => {
                        choices.insert(name.clone(), "skip".into());
                    }
                    KeyCode::Char('u') => {
                        choices.remove(name);
                    }
                    KeyCode::Char('A') => {
                        for n in &names {
                            choices.entry(n.clone()).or_insert_with(|| "source".into());
                        }
                    }
                    KeyCode::Enter => {
                        if names.iter().all(|n| {
                            choices
                                .get(n)
                                .is_some_and(|s| s == "local" || s == "source")
                        }) {
                            return Ok(Some(choices));
                        }
                        notice="Decide every file, or skip this folder. Pending/skipped files block application.".into();
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    }
}
pub fn review(app: &Engine, path: &Path) -> Result<bool> {
    let plan = read_json(path)?;
    let mut previous = BTreeMap::new();
    loop {
        let Some(choices) = decisions(app, &plan, &previous)? else {
            return Ok(false);
        };
        previous = choices.clone();
        let (_, _, conflicts) = merge::merge(
            &merge::entries(&plan["base"])?,
            &merge::entries(&plan["local"])?,
            &merge::entries(&plan["incoming"])?,
            mode(&plan["policy"]) == "master",
            &choices,
        )?;
        if !conflicts.is_empty() {
            eprintln!("File/directory choices conflict; review the whole subtree.");
            continue;
        }
        if confirm(&format!(
            "Apply reviewed changes to {}? A recovery copy will be kept.",
            text(&plan, "folder")?
        ))? {
            app.apply(path, &choices)?;
            return Ok(true);
        }
        if !confirm("Return to the review?")? {
            return Ok(false);
        }
    }
}
pub fn setup(app: &Engine, source: &str, folders: &[String], snapshot: Option<&str>) -> Result<()> {
    ensure!(io::stdin().is_terminal(), "Setup requires a terminal");
    let mut incomplete = Vec::new();
    for f in folders {
        let p = app.policy(f)?;
        tjik_ui::heading(&format!("Restore {f} from {source}"));
        if ["sync", "master"].contains(&mode(p)) {
            let (path, plan) = app.plan(f, source, snapshot)?;
            let receipt = optional_json(&app.baseline(f, source))?;
            if receipt["snapshot"] == plan["ref"]["snapshot"]
                && plan["changes"].as_array().is_some_and(|v| v.is_empty())
                && plan["conflicts"].as_array().is_some_and(|v| v.is_empty())
            {
                eprintln!("Already restored and verified.");
                continue;
            }
            if !review(app, &path)? {
                incomplete.push(f.clone());
            }
        } else {
            let target = tree::safe_root(&app.home, f)?;
            ensure!(app.eligible(f, &app.machine)?, "Collection not assigned");
            if target.exists() {
                eprintln!("Backup-only collection already exists. Restore into a separate directory for manual recovery.");
                incomplete.push(f.clone());
                continue;
            }
            if !confirm(&format!("Recover {f} from {source} onto this device? This may copy its credentials and identity."))?{incomplete.push(f.clone());continue}
            let r = app.restore(f, source, &target, snapshot)?;
            atomic_json(
                &app.baseline(f, source),
                &json!({"version":2,"folder":f,"source":source,"snapshot":r["snapshot"],"last_synced_at":now(),"entries":{},"recovery_only":true}),
            )?;
        }
    }
    ensure!(
        incomplete.is_empty(),
        "Bootstrap incomplete; resume setup for: {}",
        incomplete.join(", ")
    );
    Ok(())
}
