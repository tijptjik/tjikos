use serde_json::Value;
use std::{
    io::{IsTerminal, Write},
    time::Instant,
};
use tjik_ui::{colour, columns, fit, safe};
pub struct Table {
    tty: bool,
    current: bool,
    started: Instant,
    index: usize,
    total: usize,
    folder: String,
    entries: String,
    size: String,
}
impl Table {
    pub fn new(machine: &str, total: usize) -> Self {
        tjik_ui::heading(&format!("Synctjik [{machine} · {total} folders]"));
        eprintln!(
            "{}",
            colour(
                "2",
                &format!(
                    "{:<9} {:>6}  {:<width$} {:>8} {:>10} {:<10} {:>6}",
                    "STATUS",
                    "ITEM",
                    "FOLDER",
                    "ENTRIES",
                    "SIZE",
                    "RESULT",
                    "TIME",
                    width = columns().saturating_sub(56).max(12)
                )
            )
        );
        Self {
            tty: std::io::stderr().is_terminal(),
            current: false,
            started: Instant::now(),
            index: 0,
            total,
            folder: String::new(),
            entries: "—".into(),
            size: "—".into(),
        }
    }
    pub fn start(&mut self, f: &str, index: usize) {
        self.folder = safe(f);
        self.index = index;
        self.started = Instant::now();
        self.entries = "—".into();
        self.size = "—".into();
        self.current = false;
    }
    pub fn event(&mut self, phase: &str, v: &Value) {
        if let Some(n) = v["entries"].as_u64() {
            self.entries = n.to_string();
        }
        if let Some(n) = v["size"].as_u64() {
            self.size = if n >= 1048576 {
                format!("{:.1} MiB", n as f64 / 1048576.)
            } else {
                format!("{:.1} KiB", n as f64 / 1024.)
            };
        }
        let final_row = ["BACKUP", "SKIP", "FAILED"].contains(&phase);
        if !self.tty && !final_row {
            return;
        }
        let reason = v["reason"]
            .as_str()
            .unwrap_or(if final_row { "verified" } else { "running" });
        let status = format!(
            "{:<8} {}",
            phase,
            if phase == "FAILED" {
                "✗"
            } else if final_row {
                "✓"
            } else {
                "·"
            }
        );
        let line = format!(
            "{} {:>6}  {:<width$} {:>8} {:>10} {:<10} {:>5.1}s",
            colour(if phase == "FAILED" { "1;31" } else { "1;36" }, &status),
            format!("{}/{}", self.index, self.total),
            fit(&self.folder, columns().saturating_sub(58).max(12)),
            self.entries,
            self.size,
            fit(&safe(reason), 10),
            self.started.elapsed().as_secs_f64(),
            width = columns().saturating_sub(58).max(12)
        );
        if self.tty {
            eprint!("\r\x1b[2K{line}");
            if final_row {
                eprintln!();
                self.current = false;
            } else {
                self.current = true;
            }
            let _ = std::io::stderr().flush();
        } else {
            eprintln!("{line}");
        }
    }
}
impl Drop for Table {
    fn drop(&mut self) {
        if self.current {
            eprintln!();
        }
    }
}
pub fn demo() {
    let mut t = Table::new("demo", 4);
    for (i, f) in [".ssh", ".gnupg", ".local/share/icons", ".local/share/fonts"]
        .iter()
        .enumerate()
    {
        t.start(f, i + 1);
        t.event(if i==3{"SKIP"}else{"BACKUP"},&serde_json::json!({"entries":26*(i+1),"size":67379*(i+1),"reason":if i==3{"unchanged"}else{"verified"}}));
    }
}
