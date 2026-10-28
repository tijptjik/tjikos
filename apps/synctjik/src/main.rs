mod wizard;
use anyhow::{bail, ensure, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf};
use synctjik::{engine::Engine, merge, ui, util::*};
#[derive(Parser)]
#[command(
    version,
    about = "Verified archives and reviewed device state synchronization"
)]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    #[arg(long, global = true)]
    machine: Option<String>,
    #[arg(long, global = true)]
    state: Option<PathBuf>,
    #[arg(long, global = true)]
    json: bool,
    #[arg(long)]
    demo: bool,
    #[command(subcommand)]
    command: Option<Cmd>,
}
#[derive(Subcommand)]
enum Cmd {
    /// Validate policy and dependencies without restoring or backing up.
    Check,
    /// Enable the backup timer only after restoration readiness passes.
    Schedule {
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        print: bool,
    },
    /// Check that required bootstrap collections have been restored.
    Ready,
    Status {
        #[arg(long,num_args=1..,alias="targets")]
        devices: Vec<String>,
    },
    Devices {
        selectors: Vec<String>,
    },
    Backup {
        folders: Vec<String>,
        #[arg(long)]
        force: bool,
    },
    Prune {
        folders: Vec<String>,
        #[arg(long)]
        apply: bool,
    },
    #[command(alias = "sync")]
    Plan {
        folders: Vec<String>,
        #[arg(long = "from")]
        source: Option<String>,
        #[arg(long)]
        snapshot: Option<String>,
    },
    Setup {
        folders: Vec<String>,
        #[arg(long = "from")]
        source: Option<String>,
        #[arg(long)]
        snapshot: Option<String>,
    },
    Apply {
        plan: PathBuf,
        #[arg(long)]
        resolutions: Option<PathBuf>,
    },
    Review {
        plan: PathBuf,
        #[arg(long)]
        file: Option<String>,
    },
    Restore {
        folder: String,
        #[arg(long = "from")]
        source: String,
        #[arg(long)]
        to: PathBuf,
        #[arg(long)]
        snapshot: Option<String>,
    },
    History {
        folder: String,
        #[arg(long = "from")]
        source: Option<String>,
    },
    Diff {
        folder: String,
        #[arg(long)]
        left: String,
        #[arg(long)]
        right: String,
        #[arg(long)]
        file: Option<String>,
    },
    /// Declare this device's required collections without editing shared policy.
    Enroll {
        #[arg(long)]
        role: Option<String>,
        #[arg(long = "from")]
        source: Option<String>,
        #[arg(long, value_delimiter = ',')]
        collections: Vec<String>,
        #[arg(long)]
        replace: bool,
    },
}
fn print(v: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}
fn load(cli: &Cli) -> Result<(Engine, Option<Value>, PathBuf)> {
    let home = cli
        .home
        .clone()
        .unwrap_or_else(|| std::env::var_os("HOME").unwrap_or_default().into())
        .canonicalize()?;
    let config_dir = if cli.home.is_some() {
        home.join(".config")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
    };
    let transport = config_dir.join("synctjik/rclone.conf");
    if std::env::var_os("RCLONE_CONFIG").is_none() && transport.is_file() {
        std::env::set_var("RCLONE_CONFIG", transport);
    }
    let policy = cli
        .config
        .clone()
        .unwrap_or_else(|| config_dir.join("archive-policy.json"));
    let mut config = read_json(&policy)?;
    let machine = cli.machine.clone().unwrap_or_else(|| {
        std::fs::read_to_string("/proc/sys/kernel/hostname")
            .unwrap_or_default()
            .trim()
            .split('.')
            .next()
            .unwrap_or("")
            .into()
    });
    host(&machine)?;
    let profile_path = config_dir.join("synctjik/device.json");
    let profile = if profile_path.exists() {
        let p = read_json(&profile_path)?;
        ensure!(
            p["machine"] == machine,
            "Archive enrollment belongs to another device"
        );
        Some(p)
    } else {
        None
    };
    if !matches!(cli.command, Some(Cmd::Enroll { .. })) {
        if let Some(p) = &profile {
            let selected = strings(&p["collections"])?;
            ensure!(!selected.is_empty(), "No archive collections enrolled");
            let all = config["folders"]
                .as_object_mut()
                .context("Missing folders")?;
            for n in &selected {
                ensure!(
                    all.contains_key(n),
                    "Enrolled collection disappeared from policy: {n}"
                );
            }
            all.retain(|n, _| selected.contains(n));
            for v in all.values_mut() {
                v["devices"] = json!([machine]);
            }
            config["initial_sync_required"] = json!([machine]);
            config["bootstrap_source"] = p["source"].clone();
        }
    }
    let state = cli
        .state
        .clone()
        .unwrap_or_else(|| home.join(".local/state/device-archives"));
    Ok((
        Engine::new(config, home, machine, state)?,
        profile,
        profile_path,
    ))
}
fn readiness(app: &Engine, profile: Option<&Value>) -> Result<()> {
    app.ready()?;
    if let Some(p) = profile {
        let source = app.device(text(p, "source")?)?;
        let mut missing = Vec::new();
        for f in strings(&p["collections"])? {
            let r = optional_json(&app.baseline(&f, &source))?;
            if r["folder"] != f
                || r["source"] != source
                || r["version"] != 2
                || !synctjik::fs::safe_root(&app.home, &f)?.is_dir()
            {
                missing.push(f)
            }
        }
        ensure!(
            missing.is_empty(),
            "Required collections incomplete: {}",
            missing.join(", ")
        );
    }
    Ok(())
}
fn run() -> Result<()> {
    unsafe {
        libc::umask(0o077);
    }
    let cli = Cli::parse();
    if cli.demo {
        ui::demo();
        return Ok(());
    }
    let (app, profile, profile_path) = load(&cli)?;
    let _lock = app.lock()?;
    let command = cli.command.context("Choose a command; see --help")?;
    let source_default = profile
        .as_ref()
        .and_then(|p| p["source"].as_str())
        .unwrap_or(&app.master)
        .to_owned();
    match command {
        Cmd::Schedule {
            role,
            print: preview,
        } => {
            let role = role
                .or_else(|| {
                    profile
                        .as_ref()
                        .and_then(|p| p["role"].as_str())
                        .map(str::to_owned)
                })
                .context("Specify --role for a device without enrollment")?;
            if preview {
                let (service, timer) = synctjik::schedule::units(&role)?;
                print(&json!({"service":service,"timer":timer}))?;
            } else {
                readiness(&app, profile.as_ref())?;
                synctjik::schedule::install(
                    &app,
                    &role,
                    profile_path.parent().unwrap().parent().unwrap(),
                )?;
            }
        }
        Cmd::Check => {
            for f in app.folders(false)? {
                let p = app.policy(&f)?;
                if encrypted(p) {
                    text(&app.config, "recipient")?;
                    let key = expand(text(&app.config, "identity")?, &app.home);
                    ensure!(key.is_file(), "Unlock the age identity first");
                    run_dependency("age")?;
                }
            }
            if !app.store.local() {
                run_dependency("rclone")?;
            }
            print(&json!({"valid":true,"collections":app.folders(false)?.len()}))?;
        }
        Cmd::Ready => {
            readiness(&app, profile.as_ref())?;
            print(&json!({"ready":true}))?;
        }
        Cmd::Status { devices } => print(&json!(app.status(&devices)?))?,
        Cmd::Devices { selectors } => {
            for d in app.devices(&if selectors.is_empty() {
                vec!["all".into()]
            } else {
                selectors
            })? {
                let f: Vec<_> = app.config["folders"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .filter(|f| app.eligible(f, &d).unwrap_or(false))
                    .collect();
                print(&json!({"device":d,"folders":f}))?;
            }
        }
        Cmd::Backup { folders, force } => {
            readiness(&app, profile.as_ref())?;
            let folders = if folders.is_empty() {
                app.folders(false)?
            } else {
                folders
            };
            let mut table = if cli.json {
                None
            } else {
                Some(ui::Table::new(&app.machine, folders.len()))
            };
            let mut failures = 0;
            for (i, f) in folders.iter().enumerate() {
                if let Some(t) = &mut table {
                    t.start(f, i + 1);
                }
                let result = app.backup(f, force, &mut |phase, v| {
                    if let Some(t) = &mut table {
                        t.event(phase, v)
                    } else {
                        println!("{}", json!({"event":phase,"folder":f,"detail":v}));
                    }
                });
                match result {
                    Ok(_) => (),
                    Err(e) => {
                        failures += 1;
                        if let Some(t) = &mut table {
                            t.event("FAILED", &json!({"reason":"failed"}));
                        }
                        eprintln!(
                            "synctjik: {}: {}",
                            tjik_ui::safe(f),
                            tjik_ui::safe(&e.to_string())
                        );
                    }
                }
            }
            ensure!(failures == 0, "{failures} collections failed");
        }
        Cmd::Prune { folders, apply } => {
            for f in if folders.is_empty() {
                app.folders(false)?
            } else {
                folders
            } {
                print(&app.prune(&f, apply, None)?)?;
            }
        }
        Cmd::Plan {
            folders,
            source,
            snapshot,
        } => {
            let source = app.device(source.as_deref().unwrap_or(&source_default))?;
            for f in if folders.is_empty() {
                app.folders(true)?
            } else {
                folders
            } {
                let (path, p) = app.plan(&f, &source, snapshot.as_deref())?;
                print(
                    &json!({"folder":f,"plan":path,"changes":p["changes"],"conflicts":p["conflicts"]}),
                )?;
            }
        }
        Cmd::Setup {
            folders,
            source,
            snapshot,
        } => {
            let source = app.device(source.as_deref().unwrap_or(&source_default))?;
            let folders = if folders.is_empty() {
                if let Some(p) = &profile {
                    strings(&p["collections"])?
                } else {
                    app.folders(true)?
                }
            } else {
                folders
            };
            wizard::setup(&app, &source, &folders, snapshot.as_deref())?;
            readiness(&app, profile.as_ref())?;
        }
        Cmd::Apply { plan, resolutions } => {
            let decisions = if let Some(p) = resolutions {
                serde_json::from_value(read_json(&p)?)?
            } else {
                BTreeMap::new()
            };
            print(&app.apply(&plan, &decisions)?)?;
        }
        Cmd::Review { plan, file } => {
            if let Some(f) = file {
                print!("{}", wizard::preview(&app, &read_json(&plan)?, &f)?);
            } else {
                wizard::review(&app, &plan)?;
            }
        }
        Cmd::Restore {
            folder,
            source,
            to,
            snapshot,
        } => {
            let dest = expand(to.to_str().context("Invalid path")?, &app.home);
            let dest = if dest.is_absolute() {
                dest
            } else {
                std::env::current_dir()?.join(dest)
            };
            print(&app.restore(&folder, &app.device(&source)?, &dest, snapshot.as_deref())?)?;
        }
        Cmd::History { folder, source } => {
            let source = app.device(source.as_deref().unwrap_or(&source_default))?;
            print(&json!(app
                .store
                .list(&app.prefix(&folder, &source)?, true)?))?;
        }
        Cmd::Diff {
            folder,
            left,
            right,
            file,
        } => {
            let tmp = tempfile::tempdir_in(&app.state)?;
            let a = tmp.path().join("a");
            let b = tmp.path().join("b");
            std::fs::create_dir(&a)?;
            std::fs::create_dir(&b)?;
            let (_, lm) = app.fetch(&folder, &app.device(&left)?, &a, None, file.is_some())?;
            let (_, rm) = app.fetch(&folder, &app.device(&right)?, &b, None, file.is_some())?;
            if let Some(f) = file {
                print!(
                    "{}",
                    wizard::text_diff(
                        &a.join("tree"),
                        &lm["entries"],
                        &b.join("tree"),
                        &rm["entries"],
                        &f
                    )?
                );
            } else {
                let (_, changes, conflicts) = merge::merge(
                    &merge::entries(&lm["entries"])?,
                    &merge::entries(&lm["entries"])?,
                    &merge::entries(&rm["entries"])?,
                    true,
                    &BTreeMap::new(),
                )?;
                print(
                    &json!({"folder":folder,"left":left,"right":right,"changes":changes,"conflicts":conflicts}),
                )?;
            }
        }
        Cmd::Enroll {
            role,
            source,
            collections,
            replace,
        } => {
            ensure!(
                !profile_path.exists() || replace,
                "Device already enrolled; use --replace to revise"
            );
            let role = if let Some(r) = role {
                r
            } else {
                wizard::ask("Device role (desktop/laptop/server)", "desktop")?
            };
            ensure!(
                ["desktop", "laptop", "server"].contains(&role.as_str()),
                "Unknown role"
            );
            let source = if let Some(s) = source {
                s
            } else {
                wizard::ask("Restore from device", &app.master)?
            };
            let source = app.device(&source)?;
            ensure!(
                source != app.machine,
                "Choose a source other than the new device"
            );
            let mut selected = collections;
            if selected.is_empty() {
                eprintln!("Choose required collections. Credential folders copy identity; select them only when intended.");
                for (f, p) in app.config["folders"].as_object().unwrap() {
                    if ["desktop", "master-backup"].contains(&mode(p)) && app.machine != app.master
                    {
                        continue;
                    }
                    if wizard::confirm(&format!(
                        "Require {f} ({}, encrypted={})?",
                        mode(p),
                        encrypted(p)
                    ))? {
                        selected.push(f.clone());
                    }
                }
            }
            ensure!(
                !selected.is_empty(),
                "No collections selected; enrollment incomplete"
            );
            selected.sort();
            selected.dedup();
            for f in &selected {
                let p = app.policy(f)?;
                ensure!(
                    !["desktop", "master-backup"].contains(&mode(p)) || app.machine == app.master,
                    "Leader-only backup cannot be enrolled on this device: {f}"
                );
            }
            atomic_json(
                &profile_path,
                &json!({"version":1,"machine":app.machine,"role":role,"source":source,"collections":selected}),
            )?;
            eprintln!("Collections enrolled. Run synctjik setup to restore them; readiness remains blocked until complete.");
        }
    }
    Ok(())
}
fn run_dependency(name: &str) -> Result<()> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    if !std::env::split_paths(&path).any(|p| p.join(name).is_file()) {
        bail!("Missing prerequisite: {name}")
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("synctjik: {}", tjik_ui::safe(&e.to_string()));
        std::process::exit(1)
    }
}
