//! Git-only fleet commands. Share the workflow's locks without committing user work.
use anyhow::{bail, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()?;
    if !output.status.success() {
        bail!(
            "{}: git {} failed\n{}{}",
            repo.display(),
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
fn git_path(repo: &Path, name: &str) -> Result<PathBuf> {
    let path = PathBuf::from(git(repo, &["rev-parse", "--git-path", name])?);
    Ok(if path.is_absolute() {
        path
    } else {
        repo.join(path)
    })
}
fn rebase(repo: &Path) -> Result<Option<PathBuf>> {
    for name in ["rebase-merge", "rebase-apply"] {
        let path = git_path(repo, name)?;
        if path.exists() {
            if name == "rebase-apply" && !path.join("rebasing").exists() {
                bail!("{}: git am is in progress", repo.display());
            }
            return Ok(Some(path));
        }
    }
    Ok(None)
}
fn preflight(repo: &Path, sync: bool, allow_rebase: bool) -> Result<String> {
    if Path::new(&git(repo, &["rev-parse", "--show-toplevel"])?) != repo {
        bail!("expected a repository root: {}", repo.display());
    }
    let rebasing = rebase(repo)?;
    if !git(repo, &["diff", "--name-only", "--diff-filter=U"])?.is_empty() {
        bail!(
            "{}: resolve and stage conflicts before retrying",
            repo.display()
        );
    }
    if rebasing.is_some() && !allow_rebase {
        bail!(
            "{}: rebase paused; resolve it before retrying",
            repo.display()
        );
    }
    let mut markers = vec!["REVERT_HEAD", "BISECT_LOG", "index.lock"];
    if rebasing.is_none() {
        markers.extend(["MERGE_HEAD", "CHERRY_PICK_HEAD", "sequencer"]);
    }
    for name in markers {
        if git_path(repo, name)?.exists() {
            bail!("{}: unfinished Git operation ({name})", repo.display());
        }
    }
    let branch = if let Some(path) = rebasing.as_ref() {
        let branch = fs::read_to_string(path.join("head-name"))?
            .trim()
            .to_owned();
        if !branch.starts_with("refs/heads/") {
            bail!("rebase has no named branch");
        }
        if !git(repo, &["diff", "--name-only"])?.is_empty() {
            bail!("unstaged changes during rebase; stage intended resolutions first");
        }
        branch
    } else {
        git(repo, &["symbolic-ref", "--quiet", "HEAD"])?
    };
    git(
        repo,
        &[
            "rev-parse",
            "--abbrev-ref",
            &format!("{}@{{upstream}}", branch.trim_start_matches("refs/heads/")),
        ],
    )?;
    let changes = git(repo, &["status", "--porcelain"])?;
    let dirty = if rebasing.is_some() {
        !git(repo, &["ls-files", "--others", "--exclude-standard"])?.is_empty()
    } else {
        !changes.is_empty()
    };
    if sync && dirty {
        bail!(
            "{}: working tree is dirty; commit or stash before synchronizing",
            repo.display()
        );
    }
    Ok(changes)
}
pub fn cli(args: Vec<String>) -> Result<i32> {
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        println!("tjikup git <push|sync> [--dry-run] [-v|--verbose]\n\nPush existing commits, or pull all repositories with rebase before pushing any.\nNever commits, stages, stashes, resets or force-pushes. Sync requires clean trees.\nA resolved paused rebase continues with its existing commit message.\n--dry-run performs local preflight only; no network, locks or Git changes.");
        return Ok(0);
    }
    if !["push", "sync"].contains(&args[0].as_str())
        || args[1..]
            .iter()
            .any(|s| !["--dry-run", "--verbose", "-v"].contains(&s.as_str()))
    {
        bail!("run tjikup git --help for usage");
    }
    let sync = args[0] == "sync";
    let dry = args.iter().any(|s| s == "--dry-run");
    let verbose = args.iter().any(|s| s == "--verbose" || s == "-v");
    let repos = vec![
        ("Dotjik", crate::commands::repository().canonicalize()?),
        (
            "TjikOS",
            std::env::var_os("TJIKOS_REPO")
                .map(PathBuf::from)
                .unwrap_or(crate::commands::home()?.join("code/tjikos"))
                .canonicalize()?,
        ),
    ];
    if repos[0].1 == repos[1].1 {
        bail!("repository paths must be distinct");
    }
    let mut locks = Vec::new();
    if !dry {
        let directory = PathBuf::from(git(&repos[0].1, &["rev-parse", "--absolute-git-dir"])?);
        locks.push(crate::process::Lock::acquire(
            &directory.join("tjikup-workflow.lock"),
        )?);
        let mut paths = vec![&repos[0].1, &repos[1].1];
        paths.sort();
        for path in paths {
            let dir = PathBuf::from(git(path, &["rev-parse", "--absolute-git-dir"])?);
            locks.push(crate::process::Lock::acquire(
                &dir.join("tjikup-sync.lock"),
            )?);
        }
    }
    for (name, path) in &repos {
        let changes = preflight(path, sync, true)?;
        let note = if rebase(path)?.is_some() {
            "will continue rebase with its existing commit message"
        } else if !changes.is_empty() {
            "uncommitted changes are not pushed"
        } else {
            ""
        };
        if verbose || dry || !note.is_empty() {
            println!("{name}: {} {note}", path.display());
        }
    }
    if !dry {
        for (_, path) in &repos {
            if rebase(path)?.is_some() {
                preflight(path, false, true)?;
                if !Command::new("git")
                    .arg("-C")
                    .arg(path)
                    .args(["rebase", "--continue"])
                    .env("GIT_EDITOR", "true")
                    .status()?
                    .success()
                {
                    bail!("rebase paused; resolve it before retrying");
                }
                preflight(path, false, false)?;
            }
        }
    }
    let mut stages = Vec::new();
    if sync {
        stages.push(vec!["pull", "--rebase", "--no-autostash"]);
    }
    stages.push(vec!["push"]);
    for stage in stages {
        for (_, path) in &repos {
            preflight(path, sync, dry)?;
        }
        if dry {
            for (name, _) in &repos {
                println!("{name}: git {} (parallel stage)", stage.join(" "));
            }
            continue;
        }
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = repos
                .iter()
                .map(|(name, path)| {
                    let stage = &stage;
                    scope.spawn(move || {
                        let result = git(path, stage);
                        println!(
                            "{} {name}: {}",
                            if result.is_ok() { "✓" } else { "!" },
                            stage[0]
                        );
                        if verbose {
                            if let Ok(text) = &result {
                                if !text.is_empty() {
                                    println!("{name}:\n{text}");
                                }
                            }
                        }
                        result
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join()
                        .map_err(|_| anyhow::anyhow!("Git worker failed"))
                        .and_then(|r| r)
                })
                .collect::<Vec<_>>()
        });
        let errors: Vec<_> = results
            .into_iter()
            .filter_map(Result::err)
            .map(|e| e.to_string())
            .collect();
        if !errors.is_empty() {
            bail!("{}", errors.join("\n"));
        }
    }
    Ok(0)
}
