pub use crate::report::plain;
use crate::{
    actions::State,
    config::{Config, Presentation, Recipe},
    graphics::Graphics,
    report::Event,
};
use std::{
    collections::BTreeMap,
    io::{IsTerminal, Write},
    time::Instant,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}
pub fn columns() -> usize {
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    unsafe {
        libc::ioctl(2, libc::TIOCGWINSZ, &mut size);
    }
    if size.ws_col > 0 {
        usize::from(size.ws_col)
    } else {
        80
    }
}
fn fit(text: &str, available: usize) -> String {
    if width(text) <= available {
        return text.into();
    }
    if available == 0 {
        return String::new();
    }
    let mut used = 0;
    let mut result = String::new();
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w >= available {
            break;
        }
        used += w;
        result.push(c);
    }
    result.push('…');
    result
}
// Clip generated SGR text without splitting escapes or wide characters.
fn clip_styled(text: &str, limit: usize) -> String {
    let mut chars = text.chars().peekable();
    let mut result = String::new();
    let mut used = 0;
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            result.push(c);
            if chars.peek() == Some(&'[') {
                result.push(chars.next().unwrap());
                for c in chars.by_ref() {
                    result.push(c);
                    if c == 'm' {
                        break;
                    }
                }
            }
        } else {
            let cells = c.width().unwrap_or(0);
            if used + cells > limit {
                break;
            }
            used += cells;
            result.push(c);
        }
    }
    if text.contains('\x1b') {
        result.push_str("\x1b[0m");
    }
    result
}
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
#[derive(Clone)]
struct Repo {
    label: String,
    cells: [Option<State>; 2],
    commits: [Option<u64>; 2],
    dirty_files: Option<u64>,
    running: [bool; 2],
    pending: [bool; 2],
    skipped: [bool; 2],
}
pub struct Progress {
    tty: bool,
    color: bool,
    live: bool,
    show_skips: bool,
    presentation: Presentation,
    graphics: Option<Graphics>,
    active: BTreeMap<usize, (String, Instant)>,
    repos: Vec<Repo>,
    repo_recipes: BTreeMap<usize, (usize, usize)>,
    repo_open: bool,
    repo_lines: usize,
    repo_width: usize,
    repo_last: Vec<String>,
    transient: bool,
    cursor_hidden: bool,
    frame: usize,
    section: String,
    queued: Vec<Event>,
    visible: usize,
}
impl Progress {
    pub fn new(config: &Config) -> Self {
        let tty = std::io::stderr().is_terminal() && std::env::var("TERM").as_deref() != Ok("dumb");
        let color = match std::env::var("TJIKUP_COLOR").as_deref() {
            Ok("0") => false,
            Ok("1") => true,
            _ => tty && std::env::var_os("NO_COLOR").is_none(),
        };
        let mut this = Self {
            tty,
            color,
            show_skips: config.show_skips,
            live: tty && std::env::var("TJIKUP_NO_SPIN").as_deref() != Ok("1"),
            presentation: config.presentation.clone(),
            graphics: Graphics::new(&config.presentation.icons, tty && color),
            active: BTreeMap::new(),
            repos: Vec::new(),
            repo_recipes: BTreeMap::new(),
            repo_open: false,
            repo_lines: 0,
            repo_width: 0,
            repo_last: Vec::new(),
            transient: false,
            cursor_hidden: false,
            frame: 0,
            section: String::new(),
            queued: Vec::new(),
            visible: 0,
        };
        if tty && config.presentation.splash {
            this.splash();
        }
        if config.presentation.links.is_empty() {
            let title = plain(&config.title);
            eprintln!(
                "\n{}{}",
                this.center_padding(width(&title)),
                this.paint("95;1", &title)
            );
            if !config.presentation.url.is_empty() {
                let url = plain(&config.presentation.url);
                eprintln!(
                    "{}{}",
                    this.center_padding(width(&url)),
                    this.paint("90", &url)
                );
            }
        } else {
            let links: Vec<_> = config
                .presentation
                .links
                .iter()
                .map(|link| (plain(&link.title), plain(&link.url)))
                .collect();
            let widths: Vec<_> = links
                .iter()
                .map(|(title, url)| width(title).max(width(url)))
                .collect();
            let total = widths.iter().sum::<usize>() + 4 * (links.len() - 1);
            eprintln!();
            if total <= columns() {
                for (row, style) in [(0, "95;1"), (1, "90")] {
                    let mut line = this.center_padding(total);
                    for (i, (title, url)) in links.iter().enumerate() {
                        let text = if row == 0 { title } else { url };
                        let inset = (widths[i] - width(text)) / 2;
                        line.push_str(&" ".repeat(inset));
                        line.push_str(&this.paint(style, text));
                        if i + 1 < links.len() {
                            line.push_str(&" ".repeat(widths[i] - width(text) - inset + 4));
                        }
                    }
                    eprintln!("{line}");
                }
            } else {
                for (i, (title, url)) in links.iter().enumerate() {
                    if i > 0 {
                        eprintln!();
                    }
                    eprintln!(
                        "{}{}\n{}{}",
                        this.center_padding(width(title)),
                        this.paint("95;1", title),
                        this.center_padding(width(url)),
                        this.paint("90", url)
                    );
                }
            }
        }
        for (i, r) in config.recipes.iter().enumerate() {
            if r.display.repository.is_empty() {
                continue;
            }
            let label = plain(&r.display.repository);
            let row = this
                .repos
                .iter()
                .position(|v| v.label == label)
                .unwrap_or_else(|| {
                    this.repos.push(Repo {
                        label,
                        cells: [None, None],
                        commits: [None, None],
                        dirty_files: None,
                        running: [false, false],
                        pending: [false, false],
                        skipped: [false, false],
                    });
                    this.repos.len() - 1
                });
            let col = usize::from(r.display.operation == "push");
            this.repos[row].pending[col] = true;
            this.repos[row].skipped[col] = r
                .when
                .as_ref()
                .is_some_and(|c| r.env.get(&c.environment) != Some(&c.equals));
            this.repo_recipes.insert(i, (row, col));
        }
        if !this.repos.is_empty() {
            this.heading("Repos");
            this.repo_open = true;
            if tty {
                this.draw_repos();
            }
        }
        this
    }
    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.into()
        }
    }
    fn heading(&mut self, title: &str) {
        eprintln!("\n{}\n", self.paint("94;1", title));
        self.section = title.into();
    }
    fn center_padding(&self, content_width: usize) -> String {
        " ".repeat(if self.tty {
            columns().saturating_sub(content_width) / 2
        } else {
            0
        })
    }
    fn splash(&self) {
        let splash = "\n                        _   _        ____  ____  ____\n                     __| | / |  ____|_  _||   _||_   |\n                    |__  || |__|____|/  \\ |  |_   || |\n                       |_| \\___|    |_/\\_||____|  \\__/\n";
        let mut index = 0;
        let indent = splash
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);
        let content_width = splash
            .lines()
            .map(|line| width(line).saturating_sub(indent))
            .max()
            .unwrap_or(0)
            .min(columns().saturating_sub(1));
        let padding = self.center_padding(content_width);
        // Preserve the original artwork, but avoid terminal autowrap on small panes.
        for line in splash.lines() {
            if !line.is_empty() {
                eprint!("{padding}");
            }
            for c in fit(line.get(indent..).unwrap_or(""), content_width).chars() {
                if c == ' ' {
                    eprint!(" ");
                } else {
                    eprint!(
                        "{}",
                        self.paint(
                            &format!("1;38;5;{}", [196, 208, 226, 46, 39, 129][index % 6]),
                            &c.to_string()
                        )
                    );
                    index += 1;
                }
            }
            eprintln!();
        }
        eprintln!();
    }
    fn glyph(&self, label: &str) -> String {
        if !self.tty || std::env::var("TJIKUP_ICONS").as_deref() == Ok("0") {
            return String::new();
        }
        let key = label.split(" (").next().unwrap_or(label);
        let glyph = self
            .presentation
            .glyphs
            .get(label)
            .or_else(|| self.presentation.glyphs.get(key))
            .map(String::as_str)
            .unwrap_or("↻");
        format!("{} ", plain(glyph))
    }
    fn repo_text(&self, repo: &Repo) -> String {
        let mut text = format!(
            "{}  ",
            self.paint("97", &format!("{:<8}", fit(&repo.label, 8)))
        );
        for (col, verb) in ["PULL", "PUSH"].iter().enumerate() {
            let (code, marker) = if repo.running[col] {
                ("1", SPINNER[self.frame % 10].to_string())
            } else {
                match repo.cells[col] {
                    Some(State::Done) => ("94;1", "✓".into()),
                    Some(State::Unchanged) => {
                        (if repo.skipped[col] { "90" } else { "36" }, "-".into())
                    }
                    Some(State::Failed | State::Stopped) => ("31;1", "✗".into()),
                    _ => ("90", "-".into()),
                }
            };
            let detail = if self.show_skips && repo.cells[col] == Some(State::Unchanged) {
                if repo.skipped[col] {
                    " (skipped)"
                } else {
                    " (current)"
                }
            } else {
                ""
            };
            text.push_str(&self.paint(code, &format!("{marker} {verb}{detail} ")));
        }
        let icon_width = self
            .graphics
            .as_ref()
            .map(|g| g.columns() + 1)
            .unwrap_or_else(|| width(&self.glyph(&repo.label)));
        let status = if let Some(files) = repo.dirty_files {
            self.paint("31;1", &format!("({files} files)"))
        } else {
            ["↓", "↑"]
                .iter()
                .enumerate()
                .map(|(col, arrow)| {
                    let count = repo.commits[col]
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "–".into());
                    self.paint(
                        if repo.commits[col].is_some_and(|n| n > 0) {
                            "94;1"
                        } else {
                            "90"
                        },
                        &format!("{arrow}{count}"),
                    )
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        let available = columns().saturating_sub(icon_width + 1);
        let left = clip_styled(&text, available.saturating_sub(width(&plain(&status)) + 2));
        let padding = available.saturating_sub(width(&plain(&left)) + width(&plain(&status)));
        clip_styled(&format!("{left}{}{status}", " ".repeat(padding)), available)
    }
    fn draw_repos(&mut self) {
        self.repo_width = columns();
        self.repo_last.clear();
        for repo in self.repos.clone() {
            let text = self.repo_text(&repo);
            if let Some(graphics) = &mut self.graphics {
                graphics.row(&repo.label, "", &format!(" {text}"));
            } else {
                eprintln!("{}{text}", self.glyph(&repo.label));
            }
            self.repo_last.push(text);
        }
        self.repo_lines = self.repos.len() * if self.graphics.is_some() { 2 } else { 1 };
    }
    fn patch_repos(&mut self) {
        if !self.tty || !self.repo_open {
            return;
        }
        self.clear();
        if self.repo_width != columns() {
            eprint!("\x1b[{}A\r\x1b[J", self.repo_lines);
            if let Some(graphics) = &mut self.graphics {
                graphics.size = crate::graphics::geometry().unwrap_or(graphics.size);
            }
            self.draw_repos();
            return;
        }
        for (i, repo) in self.repos.iter().enumerate() {
            let text = self.repo_text(repo);
            if self.repo_last[i] == text {
                continue;
            }
            let height = if self.graphics.is_some() { 2 } else { 1 };
            let up = self.repo_lines - i * height;
            let indent = self
                .graphics
                .as_ref()
                .map(|g| g.columns())
                .unwrap_or_else(|| width(&self.glyph(&repo.label)));
            // Only patch the text to the right of the image. Placeholders are
            // never erased by spinner updates, so sprites do not flash.
            eprint!(
                "\x1b[{up}A\x1b[{}G{}\x1b[{up}B\r",
                indent + 1,
                self.graphics
                    .as_ref()
                    .map(|g| g.text(&format!(" {text}")))
                    .unwrap_or_else(|| text.clone())
            );
            self.repo_last[i] = text;
        }
    }
    fn hide_cursor(&mut self) {
        if self.live && !self.cursor_hidden {
            eprint!("\x1b[?25l");
            self.cursor_hidden = true;
        }
    }
    fn restore_cursor(&mut self) {
        if self.cursor_hidden {
            eprint!("\x1b[?25h");
            self.cursor_hidden = false;
            let _ = std::io::stderr().flush();
        }
    }
    pub fn clear(&mut self) {
        if self.transient {
            eprint!("\r\x1b[2K");
            self.transient = false;
        }
        let _ = std::io::stderr().flush();
    }
    pub fn diagnostic(&mut self, message: &str) {
        self.clear();
        self.close_repos();
        self.restore_cursor();
        eprintln!("{message}");
    }
    pub fn start(&mut self, index: usize, recipe: &Recipe) {
        self.clear();
        if recipe.interactive {
            self.restore_cursor();
        } else {
            self.hide_cursor();
        }
        if let Some(&(row, col)) = self.repo_recipes.get(&index) {
            self.repos[row].running[col] = true;
            self.patch_repos();
        }
        if self.live && !recipe.interactive {
            self.active
                .insert(index, (plain(&recipe.label), Instant::now()));
        }
        self.tick();
    }
    pub fn finish(
        &mut self,
        index: usize,
        recipe: &Recipe,
        state: State,
        events: &[Event],
        git_commits: Option<u64>,
        dirty_files: Option<u64>,
    ) {
        self.clear();
        self.active.remove(&index);
        if let Some(&(row, col)) = self.repo_recipes.get(&index) {
            self.repos[row].running[col] = false;
            self.repos[row].pending[col] = false;
            self.repos[row].cells[col] = Some(state);
            self.repos[row].commits[col] = git_commits;
            self.repos[row].dirty_files = dirty_files.or(self.repos[row].dirty_files);
            self.patch_repos();
            if self.repos.iter().all(|r| r.pending == [false, false]) {
                self.close_repos();
            }
            return;
        }
        let mut rows = events.to_vec();
        if rows.is_empty()
            && (self.show_skips
                || matches!(state, State::Failed | State::Stopped | State::Blocked)
                || state == State::Done
                    && !recipe.display.quiet
                    && !recipe.display.reports
                    && recipe.render.is_none()
                    && !recipe.prerequisite)
        {
            let (verb, icon, note) = match state {
                State::Failed => ("FAIL", "✗", "failed"),
                State::Stopped => ("CHECK", "!", "needs attention"),
                State::Blocked => ("BLOCKED", "!", "dependency did not complete"),
                State::Unchanged
                    if recipe
                        .when
                        .as_ref()
                        .is_some_and(|c| recipe.env.get(&c.environment) != Some(&c.equals)) =>
                {
                    ("SKIP", "-", "condition not met")
                }
                State::Unchanged if recipe.prerequisite => {
                    ("REQUIRE", "✓", "prerequisite satisfied")
                }
                State::Unchanged if recipe.check.is_some() => ("SKIP", "✓", "already current"),
                State::Unchanged if recipe.autocommit.is_some() => {
                    ("SKIP", "✓", "no captured changes")
                }
                State::Unchanged if !recipe.skip_codes.is_empty() => {
                    ("SKIP", "-", "recipe returned a skip status")
                }
                State::Unchanged => ("SKIP", "✓", "already current"),
                State::Done if recipe.render.is_some() => ("CHECK", "✓", "templates prepared"),
                State::Done if recipe.files.is_some() => {
                    ("CHECK", "✓", "file application completed")
                }
                State::Done if recipe.display.reports => ("CHECK", "✓", "no status reported"),
                _ => (
                    if recipe.display.verb.is_empty() {
                        "APPLY"
                    } else {
                        &recipe.display.verb
                    },
                    "✓",
                    "",
                ),
            };
            rows.push(Event {
                icon_key: String::new(),
                verb: verb.into(),
                icon: icon.into(),
                subject: recipe.label.clone(),
                note: note.into(),
                section: String::new(),
            });
        }
        for mut event in rows {
            if recipe.prerequisite {
                event.verb = "REQUIRE".into();
            }
            event.icon_key = if recipe.display.icon.is_empty() {
                recipe.label.clone()
            } else {
                recipe.display.icon.clone()
            };
            if event.section.is_empty() {
                event.section = if !recipe.display.section.is_empty() {
                    recipe.display.section.clone()
                } else if recipe.prerequisite {
                    "Prerequisites".into()
                } else {
                    "Recipes".into()
                };
            }
            if self.repo_open {
                self.queued.push(event);
            } else {
                self.event(&event);
            }
        }
    }
    fn close_repos(&mut self) {
        if !self.repo_open {
            return;
        }
        self.clear();
        for repo in &mut self.repos {
            for col in 0..2 {
                if repo.pending[col] {
                    repo.cells[col] = Some(State::Blocked);
                    repo.pending[col] = false;
                    repo.running[col] = false;
                }
            }
        }
        self.patch_repos();
        if !self.tty {
            for repo in &self.repos {
                eprintln!("{}", self.repo_text(repo));
            }
        }
        self.repo_open = false;
        for event in std::mem::take(&mut self.queued) {
            self.event(&event);
        }
    }
    fn title(&self, label: &str) -> String {
        let (base, subtype) = label
            .split_once(" (")
            .map(|(a, b)| (a, Some(b)))
            .unwrap_or((label, None));
        let base = self
            .presentation
            .titles
            .get(base)
            .map(String::as_str)
            .unwrap_or(base);
        if let Some(subtype) = subtype {
            let words: Vec<_> = subtype
                .split(' ')
                .enumerate()
                .map(|(i, word)| {
                    if (i > 0 && ["and", "of", "for", "to", "the"].contains(&word))
                        || word.chars().any(char::is_uppercase)
                    {
                        word.to_string()
                    } else {
                        let mut c = word.chars();
                        c.next()
                            .map(|v| v.to_uppercase().to_string() + c.as_str())
                            .unwrap_or_default()
                    }
                })
                .collect();
            format!("{base} ({}", words.join(" "))
        } else {
            base.into()
        }
    }
    fn event(&mut self, event: &Event) {
        let icon_key = crate::icons::resolve(
            &event.subject,
            &event.icon_key,
            &self.presentation.icon_aliases,
            |key| {
                self.graphics
                    .as_ref()
                    .map(|graphics| graphics.has_icon(key))
                    .unwrap_or_else(|| self.presentation.glyphs.contains_key(key))
            },
        );
        self.clear();
        if self.section != event.section {
            self.heading(&event.section);
        }
        let code = match event.icon.as_str() {
            "✗" | "X" => "31",
            "!" => "33",
            _ if event.verb == "SKIP" || event.icon == "-" => "90",
            _ => "32",
        };
        let prefix = format!(
            "{} {} ",
            self.paint(&format!("{code};1"), &format!("{:<7}", fit(&event.verb, 7))),
            self.paint(code, &event.icon)
        );
        let icon_width = self
            .graphics
            .as_ref()
            .map(|g| g.columns() + 1)
            .unwrap_or_else(|| width(&self.glyph(&icon_key)));
        let available = columns().saturating_sub(11 + icon_width);
        let note = fit(&event.note, available / 2);
        let title = fit(
            &self.title(&event.subject),
            available.saturating_sub(width(&note) + if note.is_empty() { 0 } else { 2 }),
        );
        let subject = if let Some((base, subtype)) = title.split_once(" (") {
            format!(
                "{}{}",
                self.paint("97", base),
                self.paint("90", &format!(" ({subtype}"))
            )
        } else {
            self.paint("97", &title)
        };
        let gap = if note.is_empty() {
            String::new()
        } else {
            " ".repeat(available.saturating_sub(width(&title) + width(&note)))
        };
        let rest = format!("{subject}{gap}{}", self.paint("90", &note));
        if let Some(graphics) = &mut self.graphics {
            graphics.row(&icon_key, &prefix, &format!(" {rest}"));
        } else {
            eprintln!("{prefix}{}{rest}", self.glyph(&icon_key));
        }
        self.visible += 1;
    }
    pub fn tick(&mut self) {
        if !self.active.is_empty() {
            self.hide_cursor();
        }
        self.frame += 1;
        self.patch_repos();
        if !self.live || self.active.is_empty() {
            return;
        }
        self.clear();
        // One transient activity line keeps the terminal quiet. Settled results
        // have their own icons and never get repainted for animation.
        if let Some((label, started)) = self.active.values().next() {
            let note = format!("{:.1}s", started.elapsed().as_secs_f64());
            eprint!(
                "{} {} {}",
                self.paint("36", &SPINNER[self.frame % 10].to_string()),
                fit(label, columns().saturating_sub(note.len() + 5)),
                self.paint("90", &note)
            );
            self.transient = true;
        }
        let _ = std::io::stderr().flush();
    }
    pub fn complete(&mut self, attention: bool) {
        self.clear();
        self.active.clear();
        self.close_repos();
        if self.visible == 0 && !attention {
            self.event(&Event {
                icon_key: String::new(),
                verb: "CHECK".into(),
                icon: "✓".into(),
                subject: "Tjikup".into(),
                note: "No changes needed".into(),
                section: "Recipes".into(),
            });
        }
        let text = if attention {
            "NEEDS ATTENTION"
        } else {
            "ALL SYSTEMS GO"
        };
        let padding = " ".repeat(columns().saturating_sub(width(text)) / 2);
        eprint!("\n{padding}");
        let palette = [
            "255;95;95",
            "255;175;0",
            "95;255;135",
            "95;215;255",
            "95;135;255",
            "215;135;255",
        ];
        for (i, c) in text.chars().enumerate() {
            eprint!(
                "{}",
                self.paint(
                    &if attention {
                        "31;1".into()
                    } else {
                        format!("1;38;2;{}", palette[i % 6])
                    },
                    &c.to_string()
                )
            );
        }
        eprintln!("\n");
        self.restore_cursor();
    }
}
impl Drop for Progress {
    fn drop(&mut self) {
        self.clear();
        self.restore_cursor();
    }
}
