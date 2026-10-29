//! Native entry points for repository policy helpers.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn context(repo: &Path) -> Result<Value> {
    let mut config = crate::config::Config::catalog(&repo.join("tjikup.json"), &BTreeSet::new())?;
    let path = config
        .device_file
        .clone()
        .unwrap_or_else(crate::device::profile_path);
    let device: crate::device::Device = serde_json::from_slice(
        &fs::read(&path).context("Enroll this machine with tjikup new device first")?,
    )?;
    device.select(&mut config)?;
    let subscriptions: BTreeSet<_> = config.recipes.iter().map(|r| r.id.clone()).collect();
    let home = std::env::var("HOME").context("HOME is required")?;
    let name = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .context("USER is required")?;
    Ok(json!({"hostname":device.hostname,"role":device.role,
        "is_client": device.role != "server", "is_desktop":device.role == "desktop",
        "is_laptop":device.role == "laptop", "is_server":device.role == "server",
        "enrolled":true,"subscriptions":subscriptions,"user":{"name":name,"home":home},
        "repo_dir":repo,"home_source":repo.join("home"),"system_source":repo.join("system/etc"),
        "os":"linux","anthropic_model_provider":"zai"}))
}
fn expanded(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest)
    } else {
        PathBuf::from(value)
    }
}
fn executable(path: &Path) -> bool {
    path.is_file() && fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}
fn which(name: &str) -> bool {
    if name.contains('/') {
        return executable(Path::new(name));
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .any(|p| executable(&p.join(name)))
}
fn success(command: &mut Command) -> Result<bool> {
    Ok(command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success())
}
fn predicate(item: &Value, rpms: Option<&BTreeSet<String>>) -> Result<bool> {
    if let Some(v) = item.get("not") {
        return Ok(!predicate(v, rpms)?);
    }
    if let Some(v) = item.get("any") {
        for entry in v.as_array().context("any must be an array")? {
            if predicate(entry, rpms)? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    for key in ["plugin", "command", "executable", "path", "rpm", "flatpak"] {
        let Some(v) = item.get(key) else { continue };
        let v = v.as_str().context("package predicate must be a string")?;
        return Ok(match key {
            "plugin" => {
                let directory = expanded("~/.config/herdr/plugins/github");
                match fs::read_dir(directory) {
                    Ok(entries) => entries.filter_map(Result::ok).any(|p| {
                        p.file_name()
                            .to_string_lossy()
                            .starts_with(&format!("{v}-"))
                            && p.path().join("herdr-plugin.toml").is_file()
                    }),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
                    Err(e) => return Err(e.into()),
                }
            }
            "command" => which(v),
            "executable" => executable(&expanded(v)),
            "path" => expanded(v).exists(),
            "rpm" => match rpms {
                Some(set) => {
                    if set.contains(v) {
                        true
                    } else {
                        // Some manifest entries name an RPM-owned command (crontab,
                        // blueman-manager). Never accept an arbitrary PATH executable.
                        let path = Path::new("/usr/bin").join(v);
                        !v.contains('/')
                            && executable(&path)
                            && success(Command::new("rpm").args(["-qf", "--"]).arg(path))?
                    }
                }
                None => success(Command::new("rpm").args(["-q", v]))?,
            },
            "flatpak" => {
                which("flatpak") && success(Command::new("flatpak").args(["info", "--system", v]))?
            }
            _ => unreachable!(),
        });
    }
    bail!("unknown package presence predicate")
}
fn package(repo: &Path, args: &[String]) -> Result<i32> {
    let action = args
        .first()
        .context("package requires check, skip-update or run")?;
    if !["check", "skip-update", "run"].contains(&action.as_str()) {
        bail!("invalid package action");
    }
    let key = args.get(1).context("package name is required")?;
    let document: Value =
        serde_json::from_slice(&fs::read(repo.join("tjikup/package-checks.json"))?)?;
    let policy = document.get(key).context("unknown package policy")?;
    let ctx = context(repo)?;
    let role = ctx["role"].as_str().unwrap();
    let applicable = policy
        .get("roles")
        .and_then(Value::as_array)
        .is_none_or(|roles| roles.is_empty() || roles.iter().any(|r| r.as_str() == Some(role)));
    let mut checks = policy
        .get("checks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for plugin in policy
        .get("plugins")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        checks.push(json!({"plugin":plugin}));
    }
    let ecosystem = policy.get("packages").and_then(Value::as_str);
    if applicable {
        if let Some(ecosystem) = ecosystem {
            let data: Value =
                serde_json::from_slice(&fs::read(repo.join("native/tasks/data.json"))?)?;
            for group in ["common", if role == "server" { "server" } else { "client" }] {
                let values = &data["packages"][group][ecosystem];
                let buckets: Vec<_> = match values.as_object() {
                    Some(map) => map.values().collect(),
                    None => vec![values],
                };
                for names in buckets {
                    if names.is_null() {
                        continue;
                    }
                    for name in names.as_array().context("invalid package list")? {
                        let name = name
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .context("invalid package name")?;
                        checks.push(json!({ecosystem:name}));
                    }
                }
            }
        }
    }
    let rpms = if applicable && ecosystem == Some("rpm") {
        let output = Command::new("rpm")
            .args([
                "-qa",
                "--qf",
                "%{NAME}\n[%{PROVIDENAME}\n][%{OBSOLETENAME}\n]",
            ])
            .output()?;
        if !output.status.success() {
            bail!("RPM database query failed");
        }
        Some(
            String::from_utf8(output.stdout)?
                .lines()
                .map(str::to_owned)
                .collect::<BTreeSet<_>>(),
        )
    } else {
        None
    };
    let values = if applicable {
        checks
            .iter()
            .map(|c| predicate(c, rpms.as_ref()))
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    let mut ready = values.iter().all(|v| *v);
    let operation = std::env::var("TJIKUP_OPERATION").unwrap_or_else(|_| "install".into());
    if key == "python" && applicable && ready && (action == "check" || operation == "install") {
        ready = success(
            Command::new("uv")
                .args(["sync", "--check", "--offline", "--project"])
                .arg(expanded("~/.local/share/pytjik"))
                .args(["--python", "3.13", "--no-python-downloads"])
                .env("UV_PROJECT_ENVIRONMENT", expanded("~/.venv")),
        )?;
    }
    if action == "check" {
        return Ok(i32::from(!ready));
    }
    if action == "skip-update" {
        return Ok(i32::from(values.iter().any(|v| *v)));
    }
    if !["install", "update"].contains(&operation.as_str()) {
        bail!("package dispatch requires install or update");
    }
    if !applicable
        || operation == "install" && ready
        || operation == "update" && !values.iter().any(|v| *v)
    {
        println!(
            "SKIP    ✓ {key}  {}",
            if operation == "install" {
                "already installed"
            } else {
                "not installed"
            }
        );
        return Ok(0);
    }
    let mut command = &args[2..];
    if command.first().is_some_and(|s| s == "--") {
        command = &command[1..];
    }
    let program = command.first().context("run requires a command after --")?;
    Ok(Command::new(program)
        .args(&command[1..])
        .status()?
        .code()
        .unwrap_or(1))
}
fn assets(args: &[String]) -> Result<i32> {
    if args.len() != 3 || args[1] != "--run-dir" {
        bail!("assets <fonts|icons> --run-dir PATH");
    }
    let kind = &args[0];
    let (title, steps): (&str, &[&str]) = match kind.as_str() {
        "fonts" => ("Fonts", &["home-run_after_72-refresh-user-fonts.fish"]),
        "icons" => (
            "Rose Punk icons",
            &[
                "home-run_after_36-tjikup-icon-cache.fish",
                "home-run_after_40-rose-punk-icons.fish",
            ],
        ),
        _ => bail!("unknown asset kind"),
    };
    let directory = Path::new(&args[2]);
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(directory.join(format!("{kind}-steps.log")))?;
    let mut changed = false;
    for (index, step) in steps.iter().enumerate() {
        let output = Command::new("fish")
            .arg("--no-config")
            .arg(directory.join(step))
            .env(
                "TJIKUP_REPORT_FILE",
                directory.join(format!("{kind}-step-{index}.report")),
            )
            .output()?;
        log.write_all(&output.stdout)?;
        log.write_all(&output.stderr)?;
        log.flush()?;
        if !output.status.success() {
            println!("FAIL    ✗ {title}  setup failed; see private step log");
            return Ok(output.status.code().unwrap_or(1));
        }
        let report = fs::read_to_string(directory.join(format!("{kind}-step-{index}.report")))
            .unwrap_or_default();
        changed |= crate::report::parse(&report, &String::from_utf8_lossy(&output.stdout), false)
            .iter()
            .any(|event| {
                event.visible()
                    && ["UPDATE", "INSTALL", "CONFIG", "SYNC", "APPLY"]
                        .contains(&event.verb.as_str())
            });
    }
    println!(
        "{}    ✓ {title}  {}",
        if changed { "APPLY" } else { "SKIP" },
        if changed { "updated" } else { "current" }
    );
    Ok(0)
}
fn repositories(repo: &Path, args: &[String]) -> Result<i32> {
    if args.len() != 1 || !["check", "apply"].contains(&args[0].as_str()) {
        bail!("repositories check|apply");
    }
    let entries: Vec<Value> =
        serde_json::from_slice(&fs::read(repo.join("native/home/repositories.json"))?)?;
    let host = crate::device::hostname()?;
    let mut missing = Vec::new();
    for entry in entries {
        if !entry["hosts"]
            .as_array()
            .context("repository hosts required")?
            .iter()
            .any(|v| v == &host)
        {
            continue;
        }
        let relative = Path::new(entry["path"].as_str().context("repository path required")?);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| c == std::path::Component::ParentDir)
        {
            bail!("repository path must be within home");
        }
        let target = expanded("~/").join(relative);
        if target.exists() {
            let result = Command::new("git")
                .arg("-C")
                .arg(&target)
                .args(["rev-parse", "--show-toplevel"])
                .output()?;
            if !result.status.success()
                || Path::new(String::from_utf8(result.stdout)?.trim()) != target.canonicalize()?
            {
                bail!("existing path is not the declared Git checkout");
            }
        } else {
            missing.push((
                target,
                entry["url"]
                    .as_str()
                    .context("repository URL required")?
                    .to_owned(),
            ));
        }
    }
    if args[0] == "check" {
        return Ok(i32::from(!missing.is_empty()));
    }
    for (target, url) in &missing {
        if !Command::new("git")
            .args(["clone", "--", url])
            .arg(target)
            .status()?
            .success()
        {
            bail!("repository clone failed");
        }
    }
    println!(
        "{}: {} home repositories created",
        if missing.is_empty() { "SKIP" } else { "APPLY" },
        missing.len()
    );
    Ok(0)
}
fn verify(repo: &Path, args: &[String]) -> Result<i32> {
    if args.len() != 1 {
        bail!("verify requires an application name");
    }
    let app = &args[0];
    let status = crate::workflow_files::run(
        repo,
        &[
            "home-files".into(),
            "check".into(),
            "--group".into(),
            app.clone(),
        ],
    )?;
    if status != 0 {
        println!("CHECK   ! {app}  needs attention; run tjikup --apply-dotfiles");
        return Ok(status);
    }
    println!("CHECK   ✓ {app}  verified");
    let units: &[&str] = match app.as_str() {
        "notjik" => &["notjik.service", "notjik-telegram.service"],
        "bartjik" => &["bartjik.service"],
        "hyprvoice" => &["hyprvoice.service"],
        "codex-lb" => &["codex-lb.service"],
        _ => &[],
    };
    if units.is_empty() {
        return Ok(0);
    }
    let command = Command::new(std::env::current_exe()?)
        .args(["workflow", "--repo"])
        .arg(repo)
        .arg("services")
        .args(units.iter().flat_map(|u| ["--unit", *u]))
        .env("TJIKUP_HIDE_UNCHANGED_SERVICES", "1")
        .env("TJIKUP_SERVICE_SECTION", "Verification")
        .status()?;
    Ok(command.code().unwrap_or(1))
}
pub fn cli(args: Vec<String>) -> Result<i32> {
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        println!("tjikup workflow [--repo PATH] COMMAND\n\n  context | role | configure\n  files <home|home-files|credentials|system> <check|plan|diff|apply> [--group NAME]\n  bootstrap-archives      Prepare missing archive policy and private transport\n  package <check|skip-update|run> NAME [-- COMMAND ...]\n  repositories <check|apply>\n  bitwarden [--upgrade]\n  assets <fonts|icons> --run-dir PATH\n  services [--check] [--unit NAME] [--exclude-unit NAME]\n  activate <bartjik|notjik>\n  verify APP\n  cache-install NAME --input PATH --output PATH -- COMMAND ...\n  capture [--dry-run]\n  fedora-release\n\nDefaults to DOTJIK_REPO or the Dotjik checkout under XDG_DATA_HOME.");
        return Ok(0);
    }
    if args == ["fedora-release"] {
        return crate::fedora_release::run();
    }
    let mut args = args;
    let repo = if args.first().is_some_and(|v| v == "--repo") {
        if args.len() < 3 {
            bail!("--repo requires a directory and command");
        }
        let repo = PathBuf::from(args.remove(1));
        args.remove(0);
        repo
    } else {
        crate::commands::repository()
    };
    match args.first().map(String::as_str) {
        Some("context" | "role" | "configure") if args.len() == 1 => {
            let value = context(&repo)?;
            match args[0].as_str() {
                "context" => println!("{}", serde_json::to_string(&value)?),
                "role" => println!("{}", value["role"].as_str().unwrap()),
                _ => (),
            };
            Ok(0)
        }
        Some("bitwarden") => crate::bitwarden_install::cli(&args[1..]),
        Some("install") => crate::installers::cli(&repo, &args[1..]),
        Some("desktop") => crate::desktop::cli(&repo, &args[1..]),
        Some("bootstrap-archives") if args.len() == 1 => crate::commands::bootstrap_archives(&repo),
        Some("files") => crate::workflow_files::run(&repo, &args[1..]),
        Some("services") => crate::workflow_services::refresh(&repo, &args[1..]),
        Some("activate") => crate::workflow_services::activate(&repo, &args[1..]),
        Some("cache-install") => crate::workflow_services::cache(&args[1..]),
        Some("capture") => crate::workflow_capture::run(&repo, &args[1..]),
        Some("repositories") => repositories(&repo, &args[1..]),
        Some("verify") => verify(&repo, &args[1..]),
        Some("package") => package(&repo, &args[1..]),
        Some("fedora-release") => crate::fedora_release::run(),
        Some("assets") => assets(&args[1..]),
        _ => bail!("unknown workflow command; run tjikup workflow --help"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_predicates_and_invalid_input() {
        let rpms = BTreeSet::from(["fish".into()]);
        assert!(predicate(
            &json!({"any":[{"rpm":"absent"},{"not":{"rpm":"missing"}}]}),
            Some(&rpms)
        )
        .unwrap());
        assert!(!predicate(&json!({"not":{"rpm":"fish"}}), Some(&rpms)).unwrap());
        assert!(predicate(&json!({"what":"unknown"}), None).is_err());
    }
}
