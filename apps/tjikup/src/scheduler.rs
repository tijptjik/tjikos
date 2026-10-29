use crate::{
    actions::{Contextual, DirtyRepository, State, UntrackedCaptureFiles},
    config::Config,
    process,
};
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::DirBuilder;
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::{Duration, Instant};

#[derive(Debug, Serialize)]
pub struct Outcome {
    #[serde(skip)]
    pub events: Vec<crate::report::Event>,
    pub state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commits: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dirty_files: Option<u64>,
    pub seconds: f64,
    pub started: f64,
    pub dependency_wait: f64,
    pub resource_wait: f64,
    pub worker_wait: f64,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    pub log: PathBuf,
}

#[derive(Serialize)]
pub struct Receipt {
    pub version: u32,
    pub title: String,
    pub interrupted: bool,
    pub seconds: f64,
    pub recipes: BTreeMap<String, Outcome>,
}
impl Receipt {
    pub fn exit_code(&self) -> i32 {
        if self.interrupted {
            130
        } else if self.recipes.values().any(|r| r.state == State::Failed) {
            1
        } else {
            0
        }
    }
}

pub fn state_root() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
        })
        .join("tjikup")
}

// Display groups are execution phases, but an earlier phase may need a task
// owned by a later group. Include that task's dependency closure rather than
// inventing edges that would introduce cycles into an otherwise valid graph.
pub(crate) fn group(recipe: &crate::config::Recipe) -> String {
    if recipe.prerequisite {
        "Prerequisites".into()
    } else if !recipe.display.repository.is_empty() {
        "Repositories".into()
    } else if !recipe.display.section.is_empty() {
        recipe.display.section.clone()
    } else {
        "Recipes".into()
    }
}
pub(crate) fn group_rank(section: &str) -> usize {
    [
        "Prerequisites",
        "Capture settings",
        "Repositories",
        "Preparation",
        "Install applications",
        "Configuration",
        "Services",
        "Recipes",
        "Package updates",
        "Verification",
        "Other devices and subscriptions",
    ]
    .iter()
    .position(|s| *s == section)
    .unwrap_or(10)
}
fn phase_work(config: &Config, phase: &str, pending: &BTreeSet<usize>) -> BTreeSet<usize> {
    let indices: BTreeMap<_, _> = config
        .recipes
        .iter()
        .enumerate()
        .map(|(i, r)| (r.id.as_str(), i))
        .collect();
    let mut work = BTreeSet::new();
    let mut visit: Vec<_> = pending
        .iter()
        .copied()
        .filter(|i| group(&config.recipes[*i]) == phase)
        .collect();
    while let Some(i) = visit.pop() {
        if !pending.contains(&i) || !work.insert(i) {
            continue;
        }
        for need in &config.recipes[i].needs {
            if let Some(&parent) = indices.get(need.as_str()) {
                visit.push(parent);
            }
        }
    }
    work
}

pub fn run(config: Config, cancelled: Arc<AtomicBool>) -> Result<(Receipt, PathBuf)> {
    let root = state_root();
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root.join("logs"))?;
    let mut paths = config.locks.clone();
    paths.sort();
    paths.dedup();
    let mut locks = Vec::new();
    // Same config through any symlink uses the same default lock.
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for path in config.snapshots.keys() {
        path.hash(&mut hash);
    }
    locks.push(process::Lock::acquire(
        &root.join(format!("workflow-{:016x}.lock", hash.finish())),
    )?);
    for path in paths {
        locks.push(process::Lock::acquire(Path::new(&path))?);
    }
    config.unchanged()?;
    let directory = tempfile::Builder::new()
        .prefix("run-")
        .tempdir_in(root.join("logs"))?
        .keep();
    let prepared = tempfile::Builder::new()
        .prefix("prepared-")
        .tempdir_in(&directory)?;
    let run_dir = prepared.path().to_path_buf();
    // Allocate all log files before any worker starts. An I/O error must not
    // unwind the scheduler while other threads still own mutating subprocesses.
    let mut logs = BTreeMap::new();
    for (i, recipe) in config.recipes.iter().enumerate() {
        process::private_file(&run_dir.join(format!("{}.report", recipe.id)))?;
        logs.insert(
            i,
            process::private_file(&directory.join(format!("{}.log", recipe.id)))?,
        );
    }
    let config = Arc::new(config);
    let started = Instant::now();
    let mut last_tick = started;
    let mut pending: BTreeSet<_> = (0..config.recipes.len()).collect();
    let mut running = BTreeMap::new();
    let mut held = BTreeSet::new();
    let mut results: BTreeMap<String, Outcome> = BTreeMap::new();
    let mut waits = vec![[0.0; 3]; config.recipes.len()];
    let mut reasons = vec![0; config.recipes.len()];
    let (send, receive) = mpsc::channel();
    let (status_send, status_receive) = mpsc::channel::<(usize, String)>();
    let mut stop = false;
    let mut exclusive_running = false;
    let mut progress = crate::progress::Progress::new(&config);
    let mut groups: Vec<_> = config.recipes.iter().map(group).collect();
    groups.sort_by_key(|name| (group_rank(name), name.clone()));
    groups.dedup();
    let mut phase = 0;
    if config.presentation.grouped_results {
        if let Some(name) = groups.first() {
            progress.enter_group(name);
        }
    }
    while !pending.is_empty() || !running.is_empty() {
        let now = Instant::now();
        for &i in &pending {
            waits[i][reasons[i]] += now.duration_since(last_tick).as_secs_f64();
        }
        last_tick = now;
        if cancelled.load(Ordering::SeqCst) {
            stop = true;
        }
        if !stop {
            if let Err(error) = config.unchanged() {
                progress.diagnostic(&format!("{error:#}"));
                // Record configuration changes as a failure of the next undispatched recipe.
                if let Some(&i) = pending.first() {
                    let r = &config.recipes[i];
                    results.insert(
                        r.id.clone(),
                        Outcome {
                            events: vec![],
                            state: State::Failed,
                            git_commits: None,
                            dirty_files: None,
                            seconds: 0.,
                            started: started.elapsed().as_secs_f64(),
                            dependency_wait: waits[i][0],
                            resource_wait: waits[i][1],
                            worker_wait: waits[i][2],
                            error: Some(error.to_string()),
                            warnings: vec![],
                            log: directory.join(format!("{}.log", r.id)),
                        },
                    );
                    pending.remove(&i);
                }
                stop = true;
            }
        }
        let allowed = if config.presentation.grouped_results && !stop {
            while phase < groups.len()
                && !pending
                    .iter()
                    .chain(running.keys())
                    .any(|i| group(&config.recipes[*i]) == groups[phase])
            {
                phase += 1;
                if let Some(name) = groups.get(phase) {
                    progress.enter_group(name);
                }
            }
            groups
                .get(phase)
                .map(|name| phase_work(&config, name, &pending))
                .unwrap_or_default()
        } else {
            pending.clone()
        };
        if !stop && !exclusive_running {
            // Configuration order is the stable tie-breaker, never BTreeMap ID order.
            for i in pending.clone() {
                if !allowed.contains(&i) {
                    continue;
                }
                if running.len() >= config.jobs {
                    break;
                }
                let r = &config.recipes[i];
                if !r.needs.iter().all(|n| {
                    results
                        .get(n)
                        .is_some_and(|o| matches!(o.state, State::Done | State::Unchanged))
                }) || r.resources.iter().any(|n| held.contains(n))
                {
                    continue;
                }
                if r.interactive && !running.is_empty() {
                    continue;
                }
                let log_path = directory.join(format!("{}.log", r.id));
                let log = logs.remove(&i).expect("validated recipe has a log");
                pending.remove(&i);
                held.extend(r.resources.iter().cloned());
                exclusive_running = r.interactive;
                progress.start(i, r);
                let (config, cancelled, run_dir, send) = (
                    config.clone(),
                    cancelled.clone(),
                    run_dir.clone(),
                    send.clone(),
                );
                let status_sender = status_send.clone();
                let offset = started.elapsed().as_secs_f64();
                let handle = std::thread::spawn(move || {
                    let begin = Instant::now();
                    let result = std::panic::catch_unwind(|| {
                        Contextual {
                            config: &config,
                            recipe: &config.recipes[i],
                            cancelled: &cancelled,
                            run_dir: &run_dir,
                            log: &log,
                            status: Some((i, status_sender.clone())),
                        }
                        .execute()
                    });
                    let mut failure_note = None;
                    let (mut state, mut error, git_commits, dirty_files) = match result {
                        Ok(Ok(completed)) => (completed.state, None, completed.git_commits, None),
                        Ok(Err(error)) => {
                            // Only typed, public diagnostics reach the terminal. Command output
                            // and configured paths remain in the private receipt/log.
                            failure_note = error
                                .downcast_ref::<UntrackedCaptureFiles>()
                                .map(UntrackedCaptureFiles::status)
                                .or_else(|| {
                                    error
                                        .downcast_ref::<crate::actions::CredentialFailure>()
                                        .map(|e| e.0.to_string())
                                });
                            let dirty_files =
                                error.downcast_ref::<DirtyRepository>().map(|e| e.files);
                            let _ = writeln!(&log, "{error:#}");
                            (State::Failed, Some(format!("{error:#}")), None, dirty_files)
                        }
                        Err(_) => (
                            State::Failed,
                            Some("recipe worker panicked".into()),
                            None,
                            None,
                        ),
                    };
                    let report = std::fs::read_to_string(
                        run_dir.join(format!("{}.report", config.recipes[i].id)),
                    )
                    .unwrap_or_default();
                    let log_text = std::fs::read_to_string(&log_path).unwrap_or_default();
                    let final_log = log_text
                        .rsplit_once(crate::actions::RETRY_BOUNDARY)
                        .map(|(_, last)| last)
                        .unwrap_or(&log_text);
                    let mut events = crate::report::parse(&report, final_log, config.show_skips);
                    if let Some(note) = failure_note {
                        events.push(crate::report::Event {
                            icon_key: String::new(),
                            verb: "FAIL".into(),
                            icon: "✗".into(),
                            subject: config.recipes[i].label.clone(),
                            note,
                            section: String::new(),
                        });
                    }
                    let mut warnings = Vec::new();
                    for event in &events {
                        if event.icon == "!" && !warnings.contains(&event.subject) {
                            warnings.push(event.subject.clone());
                        }
                        if ["✗", "X"].contains(&event.icon.as_str()) && state != State::Failed {
                            state = State::Failed;
                            error = Some(format!("task reported failure: {}", event.subject));
                        }
                    }
                    let _ = send.send((
                        i,
                        Outcome {
                            events,
                            state,
                            git_commits,
                            dirty_files,
                            seconds: begin.elapsed().as_secs_f64(),
                            started: offset,
                            dependency_wait: 0.,
                            resource_wait: 0.,
                            worker_wait: 0.,
                            error,
                            warnings,
                            log: log_path,
                        },
                    ));
                });
                running.insert(i, handle);
                if exclusive_running {
                    break;
                }
            }
        }
        for &i in &pending {
            let r = &config.recipes[i];
            reasons[i] = if !r.needs.iter().all(|n| results.contains_key(n)) {
                0
            } else if exclusive_running
                || r.interactive && !running.is_empty()
                || r.resources.iter().any(|n| held.contains(n))
            {
                1
            } else {
                2
            };
        }
        if running.is_empty() {
            if stop {
                break;
            }
            if !pending.is_empty() {
                bail!("scheduler made no progress");
            }
        } else if let Ok(first) = receive.recv_timeout(Duration::from_millis(50)) {
            // Observe every queued completion before dispatching new work, so a
            // successful peer cannot hide an already-completed failure.
            for (i, mut outcome) in std::iter::once(first).chain(receive.try_iter()) {
                if let Some(handle) = running.remove(&i) {
                    let _ = handle.join();
                }
                let r = &config.recipes[i];
                for name in &r.resources {
                    held.remove(name);
                }
                if r.interactive {
                    exclusive_running = false;
                }
                outcome.dependency_wait = waits[i][0];
                outcome.resource_wait = waits[i][1];
                outcome.worker_wait = waits[i][2];
                stop |= matches!(outcome.state, State::Failed | State::Stopped);
                progress.finish(
                    i,
                    r,
                    outcome.state,
                    &outcome.events,
                    outcome.git_commits,
                    outcome.dirty_files,
                );
                results.insert(r.id.clone(), outcome);
            }
        }
        for (index, message) in status_receive.try_iter() {
            progress.status(index, &config.recipes[index], &message);
        }
        progress.tick();
    }
    progress.clear();
    for i in pending {
        let r = &config.recipes[i];
        progress.finish(i, r, State::Blocked, &[], None, None);
        results.insert(
            r.id.clone(),
            Outcome {
                events: vec![],
                state: State::Blocked,
                git_commits: None,
                dirty_files: None,
                seconds: 0.,
                started: started.elapsed().as_secs_f64(),
                dependency_wait: waits[i][0],
                resource_wait: waits[i][1],
                worker_wait: waits[i][2],
                error: None,
                warnings: vec![],
                log: directory.join(format!("{}.log", r.id)),
            },
        );
    }
    let receipt = Receipt {
        version: 1,
        title: config.title.clone(),
        interrupted: cancelled.load(Ordering::SeqCst),
        seconds: started.elapsed().as_secs_f64(),
        recipes: results,
    };
    progress.complete(
        receipt.interrupted
            || receipt.recipes.values().any(|o| {
                matches!(o.state, State::Failed | State::Stopped | State::Blocked)
                    || !o.warnings.is_empty()
            }),
    );
    let path = directory.join("receipt.json");
    serde_json::to_writer_pretty(process::private_file(&path)?, &receipt)?;
    // Rendered scripts can contain secrets and are deleted even on failed runs.
    drop(prepared);
    Ok((receipt, path))
}
