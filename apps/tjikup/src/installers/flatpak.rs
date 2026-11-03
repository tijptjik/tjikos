//! Verified release bundles, staged with Flatpak in an isolated installation.
//!
//! Using the supported CLI keeps the statically linked runner independent of
//! libflatpak's ABI. Staging never starts the application or installs runtimes.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{path::Path, process::Command, time::Duration};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Policy {
    pub(super) app_id: String,
    #[serde(default = "stable")]
    pub(super) branch: String,
    #[serde(default = "system")]
    pub(super) installation: String,
}

#[derive(Debug, Serialize)]
pub(super) struct Installed {
    pub(super) version: String,
    pub(super) reference: String,
    pub(super) commit: String,
}

fn stable() -> String {
    "stable".into()
}
fn system() -> String {
    "system".into()
}

fn checked(command: &mut Command, purpose: &str) -> Result<String> {
    command.env("LC_ALL", "C");
    let output = crate::command_io::capture(command, Duration::from_secs(1800))
        .with_context(|| format!("{purpose} failed"))?;
    if output.code != 0 {
        // Flatpak's diagnostics identify missing runtimes and invalid bundles.
        let detail: String = output.stderr.chars().take(4096).collect();
        bail!("{purpose} failed (exit {}): {}", output.code, detail.trim());
    }
    Ok(output.stdout)
}

fn command(stage: Option<&Path>) -> Command {
    let mut cmd = Command::new("flatpak");
    if let Some(stage) = stage {
        cmd.env("FLATPAK_USER_DIR", stage.join("flatpak"))
            .env("XDG_CONFIG_HOME", stage.join("config"))
            .env("XDG_DATA_HOME", stage.join("data"))
            .env("XDG_CACHE_HOME", stage.join("cache"))
            .env("XDG_STATE_HOME", stage.join("state"));
    }
    cmd
}

impl Policy {
    fn validate(&self) -> Result<()> {
        let components: Vec<_> = self.app_id.split('.').collect();
        if components.len() < 3
            || components.iter().any(|part| {
                part.is_empty()
                    || !part
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            })
            || self.branch.is_empty()
            || self.branch.starts_with('-')
            || !self
                .branch
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            || !["system", "user"].contains(&self.installation.as_str())
        {
            bail!("invalid Flatpak app ID, branch, or installation scope");
        }
        Ok(())
    }

    fn reference(&self) -> String {
        format!(
            "app/{}/{}/{}",
            self.app_id,
            std::env::consts::ARCH,
            self.branch
        )
    }

    fn scope(&self, stage: Option<&Path>) -> &str {
        if stage.is_some() || self.installation == "user" {
            "--user"
        } else {
            "--system"
        }
    }

    fn inspect(&self, stage: Option<&Path>) -> Result<Option<Installed>> {
        self.validate()?;
        let expected = self.reference();
        let output = checked(
            command(stage).args(["list", self.scope(stage), "--app", "--columns=ref,version"]),
            "Flatpak installation query",
        )?;
        let mut found = None;
        let mut count = 0;
        for row in output.lines().filter(|line| !line.trim().is_empty()) {
            count += 1;
            // Manually installed unrelated apps may have no AppStream version.
            // Only the selected ref needs a version for a safe comparison.
            let (reference, version) = row.split_once('\t').unwrap_or((row, ""));
            let reference = if reference.starts_with("app/") {
                reference.to_owned()
            } else {
                format!("app/{reference}")
            };
            if reference == expected {
                if found.is_some() {
                    bail!("duplicate installed Flatpak ref");
                }
                if super::version(version).is_none() {
                    bail!("installed Flatpak has no supported semantic version");
                }
                found = Some(version.to_owned());
            }
        }
        if stage.is_some() && (count != 1 || found.is_none()) {
            bail!("Flatpak bundle app ID, architecture, or branch does not match policy");
        }
        let Some(version) = found else {
            return Ok(None);
        };
        let identity = checked(
            command(stage).args([
                "info",
                self.scope(stage),
                "--show-ref",
                "--show-commit",
                &format!("--arch={}", std::env::consts::ARCH),
                &self.app_id,
                &self.branch,
            ]),
            "Flatpak identity query",
        )?;
        let fields: Vec<_> = identity.split_whitespace().collect();
        if fields.len() != 2
            || fields[0] != expected
            || fields[1].len() != 64
            || !fields[1].bytes().all(|c| c.is_ascii_hexdigit())
        {
            bail!("Flatpak deployed ref or commit does not match policy");
        }
        Ok(Some(Installed {
            version,
            reference: expected,
            commit: fields[1].to_ascii_lowercase(),
        }))
    }

    pub(super) fn installed(&self) -> Result<Option<Installed>> {
        self.inspect(None)
    }

    pub(super) fn needs_update(&self, expected_version: &str) -> Result<bool> {
        let expected =
            super::version(expected_version).context("invalid Flatpak release version")?;
        Ok(self.installed()?.is_none_or(|installed| {
            // inspect() already rejects unknown versions: never guess a downgrade.
            super::version(&installed.version).is_some_and(|current| current < expected)
        }))
    }

    pub(super) fn install(
        &self,
        bundle: &Path,
        expected_version: &str,
    ) -> Result<Option<Installed>> {
        self.validate()?;
        let expected =
            super::version(expected_version).context("invalid Flatpak release version")?;
        let bundle = bundle.canonicalize().context("Flatpak bundle missing")?;
        if !bundle.is_file() {
            bail!("Flatpak bundle must be a regular file");
        }
        let stage = tempfile::tempdir()?;
        // No real remotes, runtime dependencies, or application data are touched.
        // --no-pull prevents a bundle's runtime-repository hint from causing downloads.
        checked(
            command(Some(stage.path()))
                .args([
                    "install",
                    "--user",
                    "--bundle",
                    "--no-deps",
                    "--no-related",
                    "--no-pull",
                    "--assumeyes",
                    "--noninteractive",
                ])
                .arg(&bundle),
            "Flatpak bundle staging",
        )?;
        let candidate = self
            .inspect(Some(stage.path()))?
            .context("staged Flatpak missing")?;
        if super::version(&candidate.version) != Some(expected) {
            bail!("Flatpak bundle version does not match GitHub release");
        }
        // Re-read after staging in case another updater or the user changed the
        // live deployment. Update-only runs must not reinstall a removed app;
        // install-only runs must not update an app installed in the meantime.
        let current = self.installed()?;
        let operation = std::env::var("TJIKUP_OPERATION").unwrap_or_default();
        if (operation == "update" && current.is_none())
            || (operation == "install" && current.is_some())
        {
            return Ok(None);
        }
        if let Some(current) = current {
            if super::version(&current.version).is_some_and(|v| v >= expected) {
                return Ok(None);
            }
        }
        let mut apply = if self.installation == "system" {
            let mut cmd = Command::new("sudo");
            cmd.args(["-n", "--", "flatpak"]);
            cmd
        } else {
            Command::new("flatpak")
        };
        checked(
            apply
                .args([
                    "install",
                    self.scope(None),
                    "--bundle",
                    "--or-update",
                    "--assumeyes",
                    "--noninteractive",
                ])
                .arg(&bundle),
            "Flatpak bundle installation",
        )?;
        let deployed = self
            .installed()?
            .context("Flatpak installation did not deploy the app")?;
        if deployed.reference != candidate.reference
            || deployed.commit != candidate.commit
            || super::version(&deployed.version) != Some(expected)
        {
            bail!("Flatpak post-install verification failed: deployed ref, version, or commit differs");
        }
        Ok(Some(deployed))
    }
}
