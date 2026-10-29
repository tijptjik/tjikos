mod actions;
mod config;
mod demo;
mod device;
mod graphics;
mod icons;
mod managed_files;
mod notifications;
mod process;
mod progress;
mod report;
mod scheduler;
mod selection;
mod templates;

use anyhow::{bail, Context, Result};
use config::Config;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc};

fn default_config() -> PathBuf {
    std::env::var_os("TJIKUP_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
                })
                .join("tjikup/config.json")
        })
}

fn cli() -> Result<i32> {
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().is_some_and(|s| s == "sync") {
        args.next();
        use std::os::unix::process::CommandExt;
        return Err(std::process::Command::new("synctjik")
            .args(args)
            .exec()
            .into());
    }
    if args.peek().is_some_and(|s| s == "new") {
        args.next();
        return device::cli(args.collect(), default_config());
    }
    if args.peek().is_some_and(|s| s == "files") {
        args.next();
        return Ok(managed_files::cli(args.collect()));
    }
    if args.peek().is_some_and(|s| s == "notify") {
        args.next();
        if args.next().as_deref() != Some("--stdin") || args.next().is_some() {
            bail!("usage: tjikup notify --stdin");
        }
        let mut input = Vec::new();
        std::io::stdin().take(16385).read_to_end(&mut input)?;
        if input.len() > 16384 {
            bail!("notification too large");
        }
        return Ok(
            if notifications::native(
                &serde_json::from_slice(&input)?,
                notifications::focus().as_ref(),
            )? {
                0
            } else {
                1
            },
        );
    }
    if args.peek().is_some_and(|s| s == "capabilities") {
        args.next();
        if args.next().is_some() {
            bail!("usage: tjikup capabilities");
        }
        println!(r#"{{"encrypted_credentials":1,"credential_recovery":1}}"#);
        return Ok(0);
    }
    let mut path = default_config();
    let (mut check, mut plan) = (false, false);
    let mut jobs = None;
    let mut show_skips = false;
    let mut show_other_devices = false;
    let mut demo_mode = None;
    let mut flags = BTreeSet::new();
    let mut selection = selection::Selection::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("tjikup [--config PATH] [--check | --plan | --dry-run] [--jobs N] [workflow flags]\n\nRun a version 2 declarative recipe graph.\n  --check    Validate configuration without executing commands\n  --plan     Print dependency/resource plan without executing commands\n  --dry-run  Alias for --plan; checks and previews are not executed\n  --jobs N   Override the worker limit\n  --refresh-secrets   Refresh referenced Bitwarden secrets, bypassing the local cache\n  --show-skips   Show unchanged checks and skipped tasks with their reasons\n  --show-skips-and-other-devices   Also list excluded recipes; never execute them\n  --demo [changed|unchanged|attention]   Synthetic terminal preview\n  new device   Enroll this host and choose its role and recipe subscriptions\n  files <check|plan|apply|restore>   Native file deployment (see docs/native-files.md)\n  sync <command>   Delegate archival and restoration to synctjik\n  notify --stdin   Deliver validated display metadata to local notjikctl");
                println!("\nRecipe selection (combinable):\n  --apply-dotfiles          Home configuration and its reloads only\n  --apply-sysfiles          System configuration and its reloads only\n  --apply-package-updates   Updates of installed applications only\n  --skip-package-updates    Install missing applications and configure; no updates\nSelected modes use local checks for installation dependencies; they never run installers or repository synchronization.");
                return Ok(0);
            }
            "--version" => {
                println!("tjikup {} (recipes v2)", env!("CARGO_PKG_VERSION"));
                return Ok(0);
            }
            "--config" => path = PathBuf::from(args.next().context("--config needs a path")?),
            "--jobs" => {
                jobs = Some(
                    args.next()
                        .context("--jobs needs a number")?
                        .parse::<usize>()?,
                )
            }
            "--refresh-secrets" => managed_files::secrets::enable_refresh()?,
            "--show-skips" => show_skips = true,
            "--show-skips-and-other-devices" => {
                show_skips = true;
                show_other_devices = true;
            }
            "--apply-dotfiles" => selection.home = true,
            "--apply-sysfiles" => selection.system = true,
            "--apply-package-updates" => selection.updates = true,
            "--skip-package-updates" => selection.skip_updates = true,
            "--demo" => {
                demo_mode = Some(if args.peek().is_some_and(|arg| !arg.starts_with("--")) {
                    args.next().unwrap()
                } else {
                    "changed".into()
                });
            }
            "--check" => check = true,
            "--plan" | "--dry-run" => plan = true,
            _ if arg.starts_with("--") => {
                flags.insert(arg[2..].to_owned());
            }
            _ => bail!("unexpected argument {arg}"),
        }
    }
    if check && plan {
        bail!("choose --check or --plan");
    }
    if let Some(mode) = demo_mode {
        if check || plan || jobs.is_some() || !flags.is_empty() || selection != Default::default() {
            bail!("--demo supports only a preview mode and --show-skips");
        }
        return demo::run(&mode, show_skips);
    }
    let mut config = Config::load_selected(&path, &flags, &selection)?;
    config.show_skips = show_skips;
    config.show_other_devices = show_other_devices;
    if let Some(jobs) = jobs {
        if jobs == 0 || jobs > 256 {
            bail!("jobs must be between 1 and 256");
        }
        config.jobs = jobs;
    }
    if check {
        println!(
            "Valid: {} recipes, {} managers",
            config.recipes.len(),
            config.managers.len()
        );
        return Ok(0);
    }
    if plan {
        for recipe in &config.recipes {
            println!(
                "{} · {}\n  needs: {}\n  resources: {}{}",
                recipe.id,
                recipe.label,
                recipe.needs.join(", "),
                recipe.resources.join(", "),
                if recipe.interactive {
                    " · exclusive terminal"
                } else {
                    ""
                }
            );
            if let Some(operation) = recipe.operation {
                println!(
                    "  operation: {}{}{}",
                    operation.name(),
                    recipe
                        .scope
                        .map(|s| format!(" · {}", s.name()))
                        .unwrap_or_default(),
                    if recipe.installed_gate {
                        " · installed check only"
                    } else {
                        ""
                    }
                );
            }
        }
        return Ok(0);
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(libc::SIGINT, cancelled.clone())?;
    signal_hook::flag::register(libc::SIGTERM, cancelled.clone())?;
    // Parent must restore foreground ownership after an interactive child exits.
    unsafe {
        libc::signal(libc::SIGTTOU, libc::SIG_IGN);
    }
    let focus = notifications::focus();
    let (receipt, _path) = scheduler::run(config, cancelled)?;
    notifications::completion(&receipt, focus.as_ref());
    Ok(receipt.exit_code())
}
fn main() {
    std::process::exit(match cli() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("tjikup: {error:#}");
            1
        }
    });
}
