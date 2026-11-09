use crate::{discovery::Entry, util::*};
use anyhow::{Context, Result};
use serde_json::json;
use std::{path::PathBuf, process::Command, sync::OnceLock};

#[derive(Clone)]
struct Emoji {
    glyph: String,
    name: String,
}

fn source() -> Result<&'static Vec<Emoji>> {
    static DATA: OnceLock<Result<Vec<Emoji>, String>> = OnceLock::new();
    DATA.get_or_init(|| load().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|e| anyhow::anyhow!(e.clone()))
}

fn load() -> Result<Vec<Emoji>> {
    let source = std::env::var_os("RUNTJIK_EMOJI_TEST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/share/ibus-typing-booster/data/emoji-test.txt.gz"));
    let bytes = if source.extension().is_some_and(|ext| ext == "gz") {
        let gzip = which("gzip").context("gzip is required to read the system emoji catalog")?;
        Command::new(gzip)
            .args(["-cd"])
            .arg(source)
            .output()?
            .stdout
    } else {
        std::fs::read(source)?
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut emojis = Vec::new();
    for line in text.lines() {
        if !line.contains("; fully-qualified") {
            continue;
        }
        let Some((codes, annotation)) = line.split_once('#') else {
            continue;
        };
        let name = annotation
            .split_whitespace()
            .skip(2)
            .collect::<Vec<_>>()
            .join(" ");
        let glyph = codes
            .trim()
            .split_whitespace()
            .filter_map(|code| u32::from_str_radix(code, 16).ok())
            .filter_map(char::from_u32)
            .collect::<String>();
        if !glyph.is_empty() && !name.is_empty() {
            emojis.push(Emoji { glyph, name });
        }
    }
    anyhow::ensure!(!emojis.is_empty(), "The system emoji catalog is empty");
    Ok(emojis)
}

fn row(glyph: &str, name: &str, tone: bool, has_tones: bool) -> Entry {
    Entry {
        id: format!("Emoji:{glyph}"),
        name: format!("{glyph}  {name}"),
        subtitle: if tone {
            "Skin tone variant".into()
        } else {
            String::new()
        },
        icon: crate::paths::command_icon("emoji"),
        provider: "Emoji".into(),
        keywords: name.into(),
        data: json!({"emoji":glyph,"base_name":name.split(": ").next().unwrap_or(name),"tone":tone,"has_tones":has_tones}),
    }
}

pub fn query(query: &str, demo: bool) -> Result<Vec<Entry>> {
    let input = query.trim().strip_prefix("/emoji").unwrap_or("").trim();
    let tones = input.strip_prefix("tones ");
    let term = tones.unwrap_or(input).trim().to_lowercase();
    if demo {
        return Ok(if tones.is_some() {
            vec![
                row("👍🏻", "thumbs up: light skin tone", true, false),
                row("👍🏽", "thumbs up: medium skin tone", true, false),
                row("👍🏿", "thumbs up: dark skin tone", true, false),
            ]
        } else if term.is_empty() {
            vec![Entry {
                id: "Emoji:hint".into(),
                name: "Search emoji by name".into(),
                subtitle: "Demo catalog · try /emoji smile".into(),
                icon: crate::paths::command_icon("emoji"),
                provider: "Emoji".into(),
                keywords: String::new(),
                data: json!({"disabled":true}),
            }]
        } else {
            vec![
                row("😀", "grinning face", false, true),
                row("👍", "thumbs up", false, true),
            ]
        });
    }
    let all = source()?;
    let mut rows = Vec::new();
    if let Some(base) = tones {
        let base = base.trim().to_lowercase();
        for emoji in all
            .iter()
            .filter(|emoji| {
                let lower = emoji.name.to_lowercase();
                lower.contains("skin tone")
                    && lower
                        .split(": ")
                        .next()
                        .is_some_and(|prefix| prefix.contains(&base))
            })
            .take(100)
        {
            rows.push(row(&emoji.glyph, &emoji.name, true, false));
        }
    } else if term.is_empty() {
        rows.push(Entry {
            id: "Emoji:hint".into(),
            name: "Search emoji by name".into(),
            subtitle: "Type a name such as smile, wave, or heart".into(),
            icon: crate::paths::command_icon("emoji"),
            provider: "Emoji".into(),
            keywords: String::new(),
            data: json!({"disabled":true}),
        });
    } else {
        let matches: Vec<_> = all
            .iter()
            .filter(|emoji| {
                emoji.name.to_lowercase().contains(&term) || emoji.glyph.contains(&term)
            })
            .take(180)
            .collect();
        for emoji in &matches {
            let base = emoji
                .name
                .split(": ")
                .next()
                .unwrap_or(&emoji.name)
                .to_lowercase();
            let has_tones = !emoji.name.contains("skin tone")
                && all.iter().any(|candidate| {
                    candidate
                        .name
                        .to_lowercase()
                        .starts_with(&(base.clone() + ": "))
                        && candidate.name.contains("skin tone")
                });
            rows.push(row(
                &emoji.glyph,
                &emoji.name,
                emoji.name.contains("skin tone"),
                has_tones,
            ));
        }
        if rows.is_empty() {
            rows.push(Entry {
                id: "Emoji:none".into(),
                name: "No matching emoji".into(),
                subtitle: "Try a shorter word or another name".into(),
                icon: crate::paths::command_icon("emoji"),
                provider: "Emoji".into(),
                keywords: String::new(),
                data: json!({"disabled":true}),
            });
        }
    }
    Ok(rows)
}
