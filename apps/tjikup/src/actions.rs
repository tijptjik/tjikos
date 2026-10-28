use crate::{
    config::{Config, Manager, Recipe},
    process,
};
use anyhow::{bail, Context, Result};
use std::fs::File;
use std::path::Path;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Done,
    Unchanged,
    Stopped,
    Failed,
    Blocked,
}

#[derive(Debug)]
pub struct DirtyRepository {
    pub files: u64,
}
impl std::fmt::Display for DirtyRepository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "dirty repository ({} files); commit or stash before synchronizing",
            self.files
        )
    }
}
impl std::error::Error for DirtyRepository {}

fn dirty_file_count(status: &[u8]) -> u64 {
    let mut records = status.split(|byte| *byte == 0);
    let mut count = 0;
    while let Some(record) = records.next() {
        if record.len() < 3 {
            continue;
        }
        count += 1;
        // Porcelain -z emits an additional original path for a rename/copy.
        if record[..2]
            .iter()
            .any(|byte| *byte == b'R' || *byte == b'C')
        {
            records.next();
        }
    }
    count
}

pub struct Completion {
    pub state: State,
    pub git_commits: Option<u64>,
}
impl From<State> for Completion {
    fn from(state: State) -> Self {
        Self {
            state,
            git_commits: None,
        }
    }
}
impl Completion {
    fn synced(commits: u64) -> Self {
        Self {
            state: if commits == 0 {
                State::Unchanged
            } else {
                State::Done
            },
            git_commits: Some(commits),
        }
    }
}

pub struct Contextual<'a> {
    pub config: &'a Config,
    pub recipe: &'a Recipe,
    pub cancelled: &'a AtomicBool,
    pub run_dir: &'a Path,
    pub log: &'a File,
}
impl Contextual<'_> {
    fn run(&self, argv: &[String]) -> Result<i32> {
        process::run(
            argv,
            self.recipe,
            self.cancelled,
            self.run_dir,
            self.log,
            None,
            None,
        )
    }
    fn capture(&self, argv: &[String]) -> Result<Vec<u8>> {
        let temp = tempfile::Builder::new()
            .prefix("capture-")
            .tempdir_in(self.run_dir)?;
        let output = temp.path().join("stdout");
        let mut recipe = self.recipe.clone();
        recipe.interactive = false;
        let status = process::run(
            argv,
            &recipe,
            self.cancelled,
            self.run_dir,
            self.log,
            Some(&output),
            None,
        )?;
        if status != 0 {
            bail!("{} exited {status}", argv[0]);
        }
        Ok(std::fs::read(output)?)
    }
    fn success(&self, argv: &[String]) -> Result<()> {
        let code = self.run(argv)?;
        if code != 0 {
            bail!("{} exited {code}", argv[0]);
        }
        Ok(())
    }
    fn manager(&self, name: &str) -> (&Manager, Vec<String>) {
        let manager = &self.config.managers[name];
        let mut argv = manager.command.clone();
        argv.extend(["--source".into(), manager.source.clone()]);
        if let Some(config) = &manager.config {
            argv.extend(["--config".into(), config.clone()]);
        }
        (manager, argv)
    }
    fn git(&self, repo: &str, args: &[&str]) -> Result<Vec<u8>> {
        let argv: Vec<String> = ["git", "--literal-pathspecs", "-C", repo]
            .into_iter()
            .chain(args.iter().copied())
            .map(str::to_owned)
            .collect();
        self.capture(&argv)
    }
    fn autocommit(&self) -> Result<State> {
        let commit = self.recipe.autocommit.as_ref().unwrap();
        let repo = &commit.repository;
        let lock = self.git(
            repo,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "tjikup-sync.lock",
            ],
        )?;
        let _lock = process::Lock::acquire(Path::new(std::str::from_utf8(&lock)?.trim()))?;
        for state in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-merge",
            "rebase-apply",
        ] {
            let path = self.git(
                repo,
                &["rev-parse", "--path-format=absolute", "--git-path", state],
            )?;
            if Path::new(std::str::from_utf8(&path)?.trim()).exists() {
                bail!("unfinished Git operation; resolve it before autocommit");
            }
        }
        for file in &commit.files {
            let tracked = self.git(repo, &["ls-files", "-z", "--error-unmatch", "--", file])?;
            let expected = format!("{file}\0");
            if tracked != expected.as_bytes() {
                bail!("autocommit requires an exact tracked file: {file}");
            }
        }
        let mut args = vec!["diff", "--name-only", "-z", "HEAD", "--"];
        args.extend(commit.files.iter().map(String::as_str));
        if self.git(repo, &args)?.is_empty() {
            return Ok(State::Unchanged);
        }
        // --only commits working-tree contents for these paths and preserves unrelated staged work.
        let mut args = vec!["commit", "--only", "-m", &commit.message, "--"];
        args.extend(commit.files.iter().map(String::as_str));
        self.git(repo, &args)?;
        Ok(State::Done)
    }
    fn sync(&self) -> Result<Completion> {
        let operation = self.recipe.git.as_ref().unwrap();
        let repo = &operation.repository;
        let directory = self.git(repo, &["rev-parse", "--absolute-git-dir"])?;
        let directory = Path::new(std::str::from_utf8(&directory)?.trim());
        let _lock = process::Lock::acquire(&directory.join("tjikup-sync.lock"))?;
        for state in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "BISECT_LOG",
            "index.lock",
            "rebase-merge",
            "rebase-apply",
        ] {
            if directory.join(state).exists() {
                bail!("unfinished Git operation ({state})");
            }
        }
        let dirty_files = dirty_file_count(&self.git(
            repo,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        )?);
        if dirty_files > 0 {
            return Err(DirtyRepository { files: dirty_files }.into());
        }
        let branch = self.git(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        let branch = std::str::from_utf8(&branch)?.trim();
        self.git(repo, &["rev-parse", "--verify", "@{upstream}"])?;
        let before = self.git(repo, &["rev-parse", "HEAD"])?;
        match operation.operation.as_str() {
            "pull-rebase" => {
                self.git(repo, &["pull", "--rebase", "--no-autostash"])?;
            }
            "pull-ff" => {
                let remote = self.git(
                    repo,
                    &["config", "--get", &format!("branch.{branch}.remote")],
                )?;
                self.git(
                    repo,
                    &[
                        "fetch",
                        "--no-recurse-submodules",
                        "--",
                        std::str::from_utf8(&remote)?.trim(),
                    ],
                )?;
                if !self
                    .git(repo, &["status", "--porcelain=v1", "--untracked-files=all"])?
                    .is_empty()
                {
                    bail!("repository changed during fetch");
                }
                if self.git(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"])?
                    != format!("{branch}\n").as_bytes()
                    || self.git(repo, &["rev-parse", "HEAD"])? != before
                {
                    bail!("branch changed during fetch");
                }
                let counts = self.git(
                    repo,
                    &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
                )?;
                let counts: Vec<usize> = std::str::from_utf8(&counts)?
                    .split_whitespace()
                    .map(str::parse)
                    .collect::<std::result::Result<_, _>>()?;
                if counts.len() != 2 {
                    bail!("invalid upstream counts");
                }
                if counts[0] != 0 && counts[1] != 0 {
                    bail!("diverged history; reconcile it before updating");
                }
                if counts[1] == 0 {
                    return Ok(Completion::synced(0));
                }
                self.git(
                    repo,
                    &["merge", "--ff-only", "--no-autostash", "@{upstream}"],
                )?;
            }
            "push" => {
                let behind = self.git(repo, &["rev-list", "--count", "HEAD..@{upstream}"])?;
                if std::str::from_utf8(&behind)?.trim() != "0" {
                    bail!("upstream commits remain; synchronize before pushing");
                }
                let ahead = self.git(repo, &["rev-list", "--count", "@{upstream}..HEAD"])?;
                let commits: u64 = std::str::from_utf8(&ahead)?.trim().parse()?;
                if commits == 0 {
                    return Ok(Completion::synced(0));
                }
                let pushed = self.git(repo, &["push", "--porcelain"])?;
                // Use the remote's actual before/after range. A stale tracking
                // ref can overstate the ahead count, even for an up-to-date push.
                let source_ref = format!("refs/heads/{branch}:");
                for line in std::str::from_utf8(&pushed)?.lines() {
                    let fields: Vec<_> = line.splitn(3, '\t').collect();
                    if fields.len() != 3 || !fields[1].starts_with(&source_ref) {
                        continue;
                    }
                    if fields[0] == "=" {
                        return Ok(Completion::synced(0));
                    }
                    if let Some(range) = fields[2].split_whitespace().next() {
                        if let Some((old, new)) =
                            range.split_once("...").or_else(|| range.split_once(".."))
                        {
                            let range = format!("{old}..{new}");
                            let count = self.git(repo, &["rev-list", "--count", &range])?;
                            return Ok(Completion::synced(
                                std::str::from_utf8(&count)?.trim().parse()?,
                            ));
                        }
                    }
                }
                // Unusual push refspecs may not report the tracked branch.
                // Preserve success without inventing a count for another ref.
                return Ok(State::Done.into());
            }
            _ => unreachable!(),
        }
        // Count newly incorporated upstream commits, not rewritten local commits
        // introduced by rebase. Use the pre-pull HEAD as the exclusion boundary.
        let range = format!("{}..@{{upstream}}", std::str::from_utf8(&before)?.trim());
        let count = self.git(repo, &["rev-list", "--count", &range])?;
        Ok(Completion {
            state: if self.git(repo, &["rev-parse", "HEAD"])? == before {
                State::Unchanged
            } else {
                State::Done
            },
            git_commits: Some(std::str::from_utf8(&count)?.trim().parse()?),
        })
    }
    pub fn execute(&self) -> Result<Completion> {
        if let Some(condition) = &self.recipe.when {
            if self.recipe.env.get(&condition.environment) != Some(&condition.equals) {
                return Ok(State::Unchanged.into());
            }
        }
        if let Some(check) = &self.recipe.check {
            match self.run(check)? {
                0 => return Ok(State::Unchanged.into()),
                1 if self.recipe.installed_gate => {
                    use std::io::Write;
                    writeln!(
                        &*self.log,
                        "CHECK   ! {}  not installed; run tjikup to install",
                        self.recipe.label
                    )?;
                    return Ok(State::Failed.into());
                }
                1 if !self.recipe.prerequisite
                    && (self.recipe.apply.is_some()
                        || self.recipe.files.is_some()
                        || self.recipe.render.is_some()
                        || self.recipe.autocommit.is_some()
                        || self.recipe.git.is_some()) => {}
                code => bail!("check exited {code}; apply was not run"),
            }
        }
        if self.recipe.autocommit.is_some() {
            return self.autocommit().map(Completion::from);
        }
        if self.recipe.git.is_some() {
            return self.sync();
        }
        if let Some(name) = &self.recipe.files {
            let (manager, mut argv) = self.manager(name);
            if let Some(template) = &manager.config_template {
                let mut render = argv.clone();
                render.extend(["execute-template".into(), "--file".into(), template.clone()]);
                if self.capture(&render)?
                    != std::fs::read(
                        manager
                            .config
                            .as_ref()
                            .context("config_template requires config")?,
                    )?
                {
                    use std::io::Write;
                    writeln!(&*self.log, "Configuration template changed; initialize the configuration manager and rerun tjikup")?;
                    return Ok(State::Stopped.into());
                }
            }
            let before = manager.config.as_ref().map(std::fs::read).transpose()?;
            argv.extend(["apply".into(), "--exclude=scripts".into()]);
            let skip = manager
                .skip_conflicts_env
                .as_ref()
                .is_some_and(|name| self.recipe.env.get(name).is_some_and(|value| value == "1"));
            if skip {
                argv.extend(["--no-tty".into(), "--interactive=false".into()]);
            }
            let apply = || -> Result<()> {
                let code = process::run(
                    &argv,
                    self.recipe,
                    self.cancelled,
                    self.run_dir,
                    self.log,
                    None,
                    skip.then_some(process::Input::Repeat("skip\n")),
                )?;
                if code != 0 {
                    bail!("file application exited {code}");
                }
                Ok(())
            };
            apply()?;
            if manager.refresh_retry
                && before != manager.config.as_ref().map(std::fs::read).transpose()?
            {
                apply()?;
            }
            return Ok(State::Done.into());
        }
        if let Some(batch) = &self.recipe.render {
            let (manager, argv) = self.manager(&batch.manager);
            let before = manager.config.as_ref().map(std::fs::read).transpose()?;
            // Render and syntax-check the entire batch before any dependent recipe can execute.
            for template in &batch.templates {
                let source = Path::new(&template.source);
                let contents = if source.extension().is_some_and(|e| e == "tmpl") {
                    let mut command = argv.clone();
                    command.extend([
                        "execute-template".into(),
                        "--file".into(),
                        template.source.clone(),
                    ]);
                    self.capture(&command)
                        .with_context(|| format!("render {}", template.name))?
                } else {
                    std::fs::read(source)?
                };
                let script = self.run_dir.join(&template.name);
                use std::io::Write;
                process::private_file(&script)?.write_all(&contents)?;
                let syntax: Vec<_> = template
                    .syntax
                    .iter()
                    .map(|s| s.replace("{script}", &script.to_string_lossy()))
                    .collect();
                self.success(&syntax)
                    .with_context(|| format!("syntax {}", template.name))?;
            }
            let after = manager.config.as_ref().map(std::fs::read).transpose()?;
            if before != after {
                bail!("manager configuration changed during rendering; rerun tjikup");
            }
            return Ok(State::Done.into());
        }
        let code = self.run(
            self.recipe
                .apply
                .as_ref()
                .context("missing apply command")?,
        )?;
        if code == 0 {
            Ok(State::Done.into())
        } else if self.recipe.skip_codes.contains(&code) {
            Ok(State::Unchanged.into())
        } else if self.recipe.stop_codes.contains(&code) {
            Ok(State::Stopped.into())
        } else {
            bail!("apply exited {code}")
        }
    }
}
