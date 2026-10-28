//! Operation selection is independent of device subscriptions and file paths.
use crate::config::Config;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Install,
    Configure,
    Update,
    Prepare,
}
impl Operation {
    pub fn name(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Configure => "configure",
            Self::Update => "update",
            Self::Prepare => "prepare",
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Home,
    System,
}
impl Scope {
    pub fn name(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::System => "system",
        }
    }
}

#[derive(Default, PartialEq)]
pub struct Selection {
    pub home: bool,
    pub system: bool,
    pub updates: bool,
    pub skip_updates: bool,
}
impl Selection {
    pub fn apply(&self, config: &mut Config) -> Result<()> {
        if self.updates && self.skip_updates {
            bail!("--apply-package-updates conflicts with --skip-package-updates");
        }
        let explicit = self.home || self.system || self.updates;
        let mut selected: BTreeSet<String> = config
            .recipes
            .iter()
            .filter(|r| {
                if explicit {
                    r.operation == Some(Operation::Update) && self.updates
                        || r.operation == Some(Operation::Configure)
                            && match r.scope {
                                Some(Scope::Home) => self.home,
                                Some(Scope::System) => self.system,
                                None => false,
                            }
                } else {
                    !(self.skip_updates && r.operation == Some(Operation::Update))
                }
            })
            .map(|r| r.id.clone())
            .collect();
        if selected.is_empty() {
            bail!("no recipes match the selected operations on this device");
        }
        let mut gates = BTreeSet::new();
        loop {
            let before = selected.len();
            for r in &config.recipes {
                if !selected.contains(&r.id) || gates.contains(&r.id) {
                    continue;
                }
                for id in &r.needs {
                    let dependency = config
                        .recipes
                        .iter()
                        .find(|d| &d.id == id)
                        .ok_or_else(|| anyhow::anyhow!("{}: missing dependency {id}", r.id))?;
                    if self.skip_updates && dependency.operation == Some(Operation::Update) {
                        bail!("{} requires update {id}; cannot skip package updates", r.id);
                    }
                    if explicit && !selected.contains(id) {
                        match dependency.operation {
                            Some(Operation::Install) if dependency.check.is_some() => { gates.insert(id.clone()); }
                            Some(Operation::Prepare) => (),
                            None if dependency.prerequisite || dependency.render.is_some()
                                || dependency.check.is_some() && dependency.apply.is_none()
                                    && dependency.git.is_none() && dependency.autocommit.is_none() && dependency.files.is_none() => (),
                            _ => bail!("{} requires unselected recipe {id}; declare an installed check or select its scope", r.id),
                        }
                    }
                    selected.insert(id.clone());
                }
            }
            if selected.len() == before {
                break;
            }
        }
        config.recipes.retain(|r| selected.contains(&r.id));
        for r in &mut config.recipes {
            // Subscription ownership is resolved before operation selection;
            // it is not an execution dependency on the parent installer.
            if r.part_of.as_ref().is_some_and(|id| !selected.contains(id)) {
                r.part_of = None;
            }
            if gates.contains(&r.id) {
                r.installed_gate = true;
                r.apply = None;
                r.render = None;
                r.files = None;
                r.git = None;
                r.autocommit = None;
                r.needs.clear();
                r.after.clear();
                r.resources.clear();
                r.interactive = false;
            }
            r.after.retain(|id| selected.contains(id));
            r.needs.extend(r.after.clone());
            if let Some(operation) = r.operation {
                r.env
                    .insert("TJIKUP_OPERATION".into(), operation.name().into());
            }
            r.env.insert(
                "TJIKUP_SELECTED_RUN".into(),
                if explicit { "1" } else { "0" }.into(),
            );
        }
        // Updates cannot consume workers, package locks or network bandwidth
        // until every selected installation/configuration task has completed.
        let early: Vec<_> = config
            .recipes
            .iter()
            .filter(|r| r.operation != Some(Operation::Update))
            .map(|r| r.id.clone())
            .collect();
        for r in &mut config.recipes {
            if r.operation == Some(Operation::Update) {
                r.needs.extend(early.clone());
            }
            r.needs.sort();
            r.needs.dedup();
        }
        config.validate()
    }
}
