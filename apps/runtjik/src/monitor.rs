use crate::discovery::Entry;
use anyhow::{Context, Result};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    fs,
    time::Instant,
};

#[derive(Clone)]
struct Process {
    pid: u32,
    parent: u32,
    start: u64,
    ticks: u64,
    cpu: f64,
    memory: u64,
    name: String,
}

#[derive(Default)]
pub struct Monitor {
    sort: String,
    previous: HashMap<u32, (u64, u64)>,
    last_sample: Option<Instant>,
    terminating: HashSet<(u32, u64)>,
}

fn row(process: &Process, depth: usize, demo: bool, terminating: bool) -> Entry {
    let id = format!("Process:{}:{}", process.pid, process.start);
    let name = format!(
        "{}{}{}",
        "  ".repeat(depth.min(8)),
        if depth > 0 { "└─ " } else { "" },
        process.name
    );
    let state = if terminating {
        "SIGTERM sent · waiting · Ctrl+Enter again kills · "
    } else {
        ""
    };
    Entry {
        id,
        name,
        subtitle: if demo {
            "Synthetic process · demo".into()
        } else {
            format!(
                "{state}CPU {:>5.1}% · MEM {:>5.1} MiB · PID {}",
                process.cpu,
                process.memory as f64 / 1_048_576.,
                process.pid
            )
        },
        icon: crate::paths::command_icon("monitor"),
        provider: "Monitor".into(),
        keywords: process.name.clone(),
        data: json!({"pid":process.pid,"start":process.start,"keep_open":true,"closing":terminating,"demo":demo}),
    }
}

fn parse_process(path: &std::path::Path) -> Option<Process> {
    let pid = path.file_name()?.to_string_lossy().parse::<u32>().ok()?;
    let stat = fs::read_to_string(path.join("stat")).ok()?;
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let name = stat.get(open + 1..close)?.to_owned();
    let fields: Vec<_> = stat.get(close + 2..)?.split_whitespace().collect();
    let value = |index: usize| fields.get(index)?.parse::<u64>().ok();
    let parent = value(1)? as u32;
    let ticks = value(11)?.saturating_add(value(12)?);
    let start = value(19)?;
    let pages = value(21)?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64;
    Some(Process {
        pid,
        parent,
        start,
        ticks,
        cpu: 0.,
        memory: pages.saturating_mul(page_size),
        name,
    })
}

fn synthetic() -> Vec<Process> {
    vec![
        Process {
            pid: 1200,
            parent: 1,
            start: 1,
            ticks: 0,
            cpu: 17.2,
            memory: 1024 * 1024 * 420,
            name: "runtjik".into(),
        },
        Process {
            pid: 1201,
            parent: 1200,
            start: 2,
            ticks: 0,
            cpu: 6.8,
            memory: 1024 * 1024 * 145,
            name: "runtjik-core".into(),
        },
        Process {
            pid: 1100,
            parent: 1,
            start: 3,
            ticks: 0,
            cpu: 2.1,
            memory: 1024 * 1024 * 96,
            name: "firefox".into(),
        },
    ]
}

impl Monitor {
    pub fn sort_by(&mut self, key: &str) {
        self.sort = if key == "memory" { "memory" } else { "cpu" }.into();
    }
    pub fn sort(&self) -> &str {
        if self.sort.is_empty() {
            "cpu"
        } else {
            &self.sort
        }
    }

    pub fn query(&mut self, query: &str, demo: bool) -> Result<(Vec<Entry>, Vec<String>)> {
        let term = query
            .trim()
            .strip_prefix("/monitor")
            .unwrap_or("")
            .trim()
            .to_lowercase();
        let mut processes = if demo {
            synthetic()
        } else {
            let mut all = Vec::new();
            for entry in fs::read_dir("/proc")
                .context("Cannot read /proc")?
                .flatten()
            {
                if let Some(process) = parse_process(&entry.path()) {
                    all.push(process);
                }
            }
            let now = Instant::now();
            let elapsed = self
                .last_sample
                .replace(now)
                .map(|last| now.duration_since(last).as_secs_f64().max(0.05))
                .unwrap_or(1.0);
            let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64;
            for process in &mut all {
                if let Some((_start, ticks)) = self
                    .previous
                    .get(&process.pid)
                    .copied()
                    .filter(|(start, _)| *start == process.start)
                {
                    process.cpu = (process.ticks.saturating_sub(ticks) as f64 / hz / elapsed
                        * 100.)
                        .min(999.9);
                }
            }
            self.previous = all.iter().map(|p| (p.pid, (p.start, p.ticks))).collect();
            all
        };
        self.terminating.retain(|(pid, start)| {
            parse_process(&std::path::PathBuf::from(format!("/proc/{pid}")))
                .is_some_and(|p| p.start == *start)
        });
        let by_pid: HashMap<u32, Process> = processes.iter().cloned().map(|p| (p.pid, p)).collect();
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for process in &processes {
            children
                .entry(process.parent)
                .or_default()
                .push(process.pid);
        }
        let metric = |pid: &u32| {
            by_pid
                .get(pid)
                .map(|p| {
                    if self.sort == "memory" {
                        p.memory as f64
                    } else {
                        p.cpu
                    }
                })
                .unwrap_or_default()
        };
        for group in children.values_mut() {
            group.sort_by(|a, b| metric(b).total_cmp(&metric(a)));
        }
        let mut ordered = Vec::new();
        let mut seen = HashSet::new();
        fn visit(
            pid: u32,
            depth: usize,
            children: &HashMap<u32, Vec<u32>>,
            by_pid: &HashMap<u32, Process>,
            seen: &mut HashSet<u32>,
            ordered: &mut Vec<(Process, usize)>,
        ) {
            if !seen.insert(pid) {
                return;
            }
            if let Some(process) = by_pid.get(&pid) {
                ordered.push((process.clone(), depth));
            }
            if let Some(kids) = children.get(&pid) {
                for child in kids {
                    visit(*child, depth + 1, children, by_pid, seen, ordered);
                }
            }
        }
        let mut roots: Vec<u32> = processes
            .iter()
            .filter(|p| p.parent == 0 || !by_pid.contains_key(&p.parent))
            .map(|p| p.pid)
            .collect();
        roots.sort_by(|a, b| metric(b).total_cmp(&metric(a)));
        for root in roots {
            visit(root, 0, &children, &by_pid, &mut seen, &mut ordered);
        }
        for process in &processes {
            if !seen.contains(&process.pid) {
                visit(process.pid, 0, &children, &by_pid, &mut seen, &mut ordered);
            }
        }
        if !term.is_empty() {
            ordered.retain(|(p, _)| p.name.to_lowercase().contains(&term));
        }
        processes.clear();
        let rows: Vec<_> = ordered
            .into_iter()
            .take(100)
            .map(|(process, depth)| {
                row(
                    &process,
                    depth,
                    demo,
                    self.terminating.contains(&(process.pid, process.start)),
                )
            })
            .collect();
        let closing = rows
            .iter()
            .filter(|row| row.data["closing"] == true)
            .map(|row| row.id.clone())
            .collect();
        Ok((rows, closing))
    }

    pub fn close(&mut self, entry: &Entry, demo: bool) -> Result<String> {
        if demo {
            return Ok("Demo mode does not signal real processes".into());
        }
        let pid = entry.data["pid"].as_u64().context("Select a process")? as u32;
        let start = entry.data["start"]
            .as_u64()
            .context("Process identity is missing")?;
        anyhow::ensure!(
            pid > 1 && pid != std::process::id(),
            "Runtjik will not signal PID 1 or itself"
        );
        let current = parse_process(&std::path::PathBuf::from(format!("/proc/{pid}")))
            .context("That process has already exited")?;
        anyhow::ensure!(
            current.start == start,
            "That PID now belongs to a different process"
        );
        let signal = if self.terminating.contains(&(pid, start)) {
            libc::SIGKILL
        } else {
            libc::SIGTERM
        };
        let result = unsafe { libc::kill(pid as i32, signal) };
        if result != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if signal == libc::SIGTERM {
            self.terminating.insert((pid, start));
        } else {
            self.terminating.remove(&(pid, start));
        }
        Ok(if signal == libc::SIGTERM {
            format!(
                "Sent SIGTERM to {} · Ctrl+Enter again to force stop",
                current.name
            )
        } else {
            format!("Sent SIGKILL to {}", current.name)
        })
    }
}
