//! Deterministic synthetic preview: no managers, hooks, Git or notifications.
use crate::{actions::State, config::Config, progress::Progress, report::Event};
use anyhow::Result;
use serde_json::json;
use std::{collections::BTreeSet, time::Duration};
pub fn run(mode: &str, show_skips: bool) -> Result<i32> {
    if !["changed", "unchanged", "attention"].contains(&mode) {
        anyhow::bail!("demo mode must be changed, unchanged or attention");
    }
    let dir = tempfile::tempdir()?;
    std::fs::write(
        dir.path().join("icons32.json"),
        include_bytes!("../tests/fixtures/icons32.json"),
    )?;
    let presentation: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/presentation.json"))?;
    let mut recipes = Vec::new();
    for repo in ["dotjik", "systjik", "tjikOS"] {
        for operation in ["pull", "push"] {
            recipes.push(json!({"id":format!("{repo}:{operation}"),"label":repo,"apply":["true"],"display":{"repository":repo,"operation":operation}}));
        }
    }
    for (id, label) in [
        ("bartjik", "Bartjik"),
        ("fonts", "Assets (fonts)"),
        ("services", "Systemd (user services)"),
    ] {
        recipes.push(json!({"id":id,"label":label,"apply":["true"],"display":{"reports":true,"section":"Recipes"}}));
    }
    let path = dir.path().join("config.json");
    std::fs::write(
        &path,
        serde_json::to_vec(
            &json!({"version":2,"title":"tjikOS","presentation":presentation,"recipes":recipes}),
        )?,
    )?;
    let mut config = Config::load(&path, &BTreeSet::new())?;
    config.show_skips = show_skips;
    let mut ui = Progress::new(&config);
    for (i, r) in config.recipes.iter().enumerate() {
        ui.start(i, r);
        for _ in 0..4 {
            std::thread::sleep(Duration::from_millis(70));
            ui.tick();
        }
        let git_commits = (i < 6).then(|| {
            if mode == "unchanged" {
                0
            } else {
                [3, 1, 0, 0, 12, 2][i]
            }
        });
        let (state, events) = if let Some(count) = git_commits {
            (
                if count > 0 {
                    State::Done
                } else {
                    State::Unchanged
                },
                vec![],
            )
        } else if mode == "unchanged" || i == 7 {
            (State::Unchanged, vec![])
        } else {
            (
                State::Done,
                vec![Event {
                    icon_key: String::new(),
                    verb: if i == 6 {
                        "UPDATE"
                    } else if mode == "attention" {
                        "CHECK"
                    } else {
                        "RESTART"
                    }
                    .into(),
                    icon: if i == 8 && mode == "attention" {
                        "!"
                    } else {
                        "✓"
                    }
                    .into(),
                    subject: r.label.clone(),
                    note: if i == 6 {
                        "updated"
                    } else if mode == "attention" {
                        "1 unit needs attention"
                    } else {
                        "1 active units refreshed"
                    }
                    .into(),
                    section: String::new(),
                }],
            )
        };
        ui.finish(i, r, state, &events, git_commits, None);
    }
    ui.complete(mode == "attention");
    Ok(0)
}
