use crate::{discovery::Entry, util::*};
use anyhow::{ensure, Context, Result};
use granit_parser::{Event, Parser};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    ops::Range,
    os::fd::AsRawFd,
    path::{Component, Path, PathBuf},
};

// Keep source spans separate from semantic values: edits must not reserialize
// unrelated Espanso fields, comments, or formatting.
#[derive(Debug)]
struct Node {
    span: Range<usize>,
    children: Vec<Node>,
    scalar: Option<String>,
    flow: bool,
    block_scalar: bool,
}

struct Document {
    text: String,
    values: Value,
    root: Node,
    aliases: bool,
}

impl Node {
    fn field(&self, name: &str) -> Option<&Node> {
        self.children
            .chunks_exact(2)
            .find(|pair| pair[0].scalar.as_deref() == Some(name))
            .map(|pair| &pair[1])
    }
}

fn parse(text: String) -> Result<Document> {
    let values: Value = serde_saphyr::from_str(&text)?;
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    let mut aliases = false;
    let mut parser = Parser::new_from_str(&text);
    while let Some(event) = parser.next_event() {
        let (event, span) = event?;
        let range = span
            .byte_range()
            .context("YAML source position unavailable")?;
        let node = match event {
            Event::MappingStart(style, _, _) | Event::SequenceStart(style, _, _) => {
                ensure!(stack.len() < 128, "Dictionary nesting is too deep");
                stack.push(Node {
                    span: range,
                    children: vec![],
                    scalar: None,
                    flow: style == granit_parser::StructureStyle::Flow,
                    block_scalar: false,
                });
                continue;
            }
            Event::MappingEnd | Event::SequenceEnd => {
                let mut node = stack.pop().context("Invalid YAML structure")?;
                node.span.end = range.end;
                node
            }
            Event::Scalar(value, style, _, _) => Node {
                span: range,
                children: vec![],
                scalar: Some(value.into_owned()),
                flow: false,
                block_scalar: matches!(
                    style,
                    granit_parser::ScalarStyle::Literal | granit_parser::ScalarStyle::Folded
                ),
            },
            Event::Alias(_) => {
                aliases = true;
                Node {
                    span: range,
                    children: vec![],
                    scalar: None,
                    flow: false,
                    block_scalar: false,
                }
            }
            _ => continue,
        };
        if let Some(parent) = stack.last_mut() {
            parent.children.push(node);
        } else {
            ensure!(
                root.is_none(),
                "Only single-document dictionaries are supported"
            );
            root = Some(node);
        }
    }
    Ok(Document {
        text,
        values,
        root: root.context("Empty dictionary")?,
        aliases,
    })
}

fn dictionary() -> PathBuf {
    std::env::var_os("RUNTJIK_EXPANSIONS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
                })
                .join("espanso/match")
        })
}

fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn entries(root: &Path) -> Result<Vec<Value>> {
    let mut rows = vec![];
    if !root.exists() {
        return Ok(rows);
    }
    let mut paths = vec![];
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("yml" | "yaml")
            )
            && !entry.file_name().to_string_lossy().starts_with('_')
        {
            paths.push(path.to_owned());
        }
    }
    paths.sort();
    for path in paths {
        let document = parse(fs::read_to_string(&path)?)
            .with_context(|| format!("Invalid dictionary {}", path.display()))?;
        if let Some(values) = document.values["matches"].as_array() {
            for (index, value) in values.iter().enumerate() {
                if let (Some(trigger), Some(expansion)) =
                    (value["trigger"].as_str(), value["replace"].as_str())
                {
                    rows.push(json!({"file":path.strip_prefix(root)?.to_string_lossy(), "index":index, "hash":hash(&document.text), "trigger":trigger, "expansion":expansion}));
                }
            }
        }
    }
    rows.sort_by_key(|row| {
        (
            s(&row["trigger"]).to_lowercase(),
            s(&row["trigger"]).to_owned(),
            s(&row["file"]).to_owned(),
            row["index"].as_u64(),
        )
    });
    Ok(rows)
}

fn write(path: &Path, text: &str) -> Result<()> {
    // Resolve managed symlinks rather than replacing the link itself.
    let path = if path.exists() {
        fs::canonicalize(path)?
    } else {
        path.to_owned()
    };
    let parent = path.parent().context("Dictionary has no parent")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    if path.exists() {
        temporary
            .as_file()
            .set_permissions(fs::metadata(&path)?.permissions())?;
    }
    temporary.write_all(text.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(&path)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn request(value: Value) -> Result<Value> {
    handle(&dictionary(), value)
}

fn handle(root: &Path, value: Value) -> Result<Value> {
    let action = s(&value["action"]);
    if action == "list" {
        return Ok(json!(entries(root)?));
    }
    ensure!(
        matches!(action, "save" | "delete"),
        "Unknown expansion action"
    );
    fs::create_dir_all(root)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(".runtjik.lock"))?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } == 0,
        "Cannot lock expansion dictionary"
    );
    let original = &value["original"];
    let trigger = s(&value["trigger"]).trim();
    let expansion = s(&value["expansion"]);
    if action == "save" {
        ensure!(
            !trigger.is_empty()
                && !trigger.chars().any(char::is_whitespace)
                && !expansion.is_empty(),
            "Enter a trigger without spaces and a nonempty expansion"
        );
        ensure!(
            !entries(root)?
                .iter()
                .any(|row| s(&row["trigger"]) == trigger
                    && (original.is_null()
                        || row["file"] != original["file"]
                        || row["index"] != original["index"])),
            "That trigger already exists"
        );
    }
    if original.is_null() {
        ensure!(action == "save", "Select an expansion to delete");
        let text = format!(
            "matches:\n  - trigger: {}\n    replace: {}\n",
            serde_json::to_string(trigger)?,
            serde_json::to_string(expansion)?
        );
        write(
            &root.join(format!("runtjik-{}.yml", uuid::Uuid::new_v4().simple())),
            &text,
        )?;
        return Ok(json!({}));
    }
    let relative = Path::new(
        original["file"]
            .as_str()
            .context("Missing dictionary path")?,
    );
    ensure!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "Invalid dictionary path"
    );
    let path = root.join(relative);
    let mut document = parse(fs::read_to_string(&path)?)?;
    ensure!(
        hash(&document.text) == s(&original["hash"]),
        "Dictionary changed; cancel and reopen the entry before editing"
    );
    let sequence = document
        .root
        .field("matches")
        .context("Dictionary has no matches")?;
    ensure!(
        !sequence.flow && !document.aliases,
        "Edit flow-style or aliased dictionaries in a YAML editor"
    );
    let index = original["index"].as_u64().context("Invalid match index")? as usize;
    let node = sequence
        .children
        .get(index)
        .context("Invalid match index")?;
    ensure!(!node.flow, "Edit flow-style dictionaries in a YAML editor");
    let mut expected = document.values.clone();
    let expected_matches = expected["matches"]
        .as_array_mut()
        .context("Invalid matches")?;
    if action == "delete" {
        expected_matches.remove(index);
        let line_start = |offset| document.text[..offset].rfind('\n').map_or(0, |i| i + 1);
        if sequence.children.len() == 1 {
            document.text.replace_range(sequence.span.clone(), "[]\n");
        } else {
            let end = sequence
                .children
                .get(index + 1)
                .map_or(sequence.span.end, |next| line_start(next.span.start));
            document
                .text
                .replace_range(line_start(node.span.start)..end, "");
        }
    } else {
        expected_matches[index]["trigger"] = json!(trigger);
        expected_matches[index]["replace"] = json!(expansion);
        let mut edits = vec![];
        for (name, value) in [("trigger", trigger), ("replace", expansion)] {
            let field = node
                .field(name)
                .context("Match requires text trigger and replace fields")?;
            ensure!(
                field.scalar.is_some(),
                "Match requires text trigger and replace fields"
            );
            let mut range = field.span.clone();
            let mut replacement = serde_json::to_string(value)?;
            if field.block_scalar {
                // The parser's block-scalar span covers content, not the header,
                // and can consume indentation belonging to the following key.
                let key = node
                    .children
                    .chunks_exact(2)
                    .find(|pair| pair[0].scalar.as_deref() == Some(name))
                    .context("Missing field")?;
                let after_key = &document.text[key[0].span.end..];
                let colon = after_key.find(':').context("Missing YAML colon")?;
                let tail = &after_key[colon + 1..];
                let leading = tail.len() - tail.trim_start_matches([' ', '\t']).len();
                range.start = key[0].span.end + colon + 1 + leading;
                ensure!(
                    document.text[range.start..].starts_with(['|', '>']),
                    "Edit tagged block scalars in a YAML editor"
                );
                let header = document.text[range.start..].lines().next().unwrap_or("");
                if let Some(comment) = header.find('#') {
                    replacement.push(' ');
                    replacement.push_str(&header[comment..]);
                }
                if let Some(newline) = document.text[..range.end].rfind('\n') {
                    if document.text[newline + 1..range.end].trim().is_empty() {
                        range.end = newline + 1;
                    }
                }
                replacement.push('\n');
            }
            edits.push((range, replacement));
        }
        edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        for (range, replacement) in edits {
            document.text.replace_range(range, &replacement);
        }
    }
    // Validate the edited document before replacing any user's file.
    ensure!(
        parse(document.text.clone())?.values == expected,
        "YAML edit changed unrelated fields; file was not written"
    );
    write(&path, &document.text)?;
    Ok(json!({}))
}

pub fn query(query: &str, demo: bool) -> Result<Vec<Entry>> {
    let entries = if demo {
        json!([{"trigger":";hello","expansion":"Hello, world!"},{"trigger":";thanks","expansion":"Thank you for your help."}])
    } else {
        request(json!({"action":"list"}))?
    };
    let filter = query
        .strip_prefix("/expand")
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let mut rows = vec![Entry {
        id: "Expand:new".into(),
        name: "New".into(),
        subtitle: "Create an expansion".into(),
        icon: "list-add".into(),
        provider: "Expand".into(),
        keywords: String::new(),
        data: json!({"keep_open":true}),
    }];
    for (index, entry) in entries
        .as_array()
        .context("Invalid expansion dictionary")?
        .iter()
        .enumerate()
    {
        if !format!("{} {}", s(&entry["trigger"]), s(&entry["expansion"]))
            .to_lowercase()
            .contains(&filter)
        {
            continue;
        }
        rows.push(Entry {
            id: format!(
                "Expand:{}:{}:{}",
                entry["file"].as_str().unwrap_or("demo"),
                entry["index"].as_u64().unwrap_or(index as u64),
                entry["hash"].as_str().unwrap_or("demo")
            ),
            name: s(&entry["trigger"]).into(),
            subtitle: s(&entry["expansion"]).into(),
            icon: "insert-text".into(),
            provider: "Expand".into(),
            keywords: String::new(),
            data: json!({"keep_open":true,"original":entry}),
        });
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn preserves_yaml_unicode_symlinks_and_permissions() -> Result<()> {
        let root = tempfile::tempdir()?;
        let backing = tempfile::tempdir()?;
        let target = backing.path().join("managed.yml");
        let initial = "# 日本語\nmatches:\n  - trigger: ';old' # trigger comment\n    replace: |\n      Hello\n      world\n    vars:\n      - name: date\n        type: date\nother: true\n";
        fs::write(&target, initial)?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640))?;
        let link = root.path().join("base.yml");
        symlink(&target, &link)?;
        let original = entries(root.path())?[0].clone();
        handle(
            root.path(),
            json!({"action":"save", "original":original, "trigger":";新", "expansion":"こんにちは\n{{date}}"}),
        )?;
        let saved = fs::read_to_string(&target)?;
        assert!(saved.starts_with("# 日本語\nmatches:\n"));
        assert!(saved.contains("# trigger comment"));
        assert!(saved.ends_with("    vars:\n      - name: date\n        type: date\nother: true\n"));
        assert!(link.is_symlink());
        assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o777, 0o640);
        assert_eq!(
            entries(root.path())?[0]["expansion"],
            "こんにちは\n{{date}}"
        );
        assert!(
            handle(root.path(), json!({"action":"delete", "original":original}))
                .unwrap_err()
                .to_string()
                .contains("changed")
        );
        Ok(())
    }

    #[test]
    fn crud_duplicates_and_path_guard() -> Result<()> {
        let root = tempfile::tempdir()?;
        for trigger in [";zulu", ";alpha"] {
            handle(
                root.path(),
                json!({"action":"save", "trigger":trigger, "expansion":"text"}),
            )?;
        }
        let rows = entries(root.path())?;
        assert_eq!(rows[0]["trigger"], ";alpha");
        assert!(handle(
            root.path(),
            json!({"action":"save", "trigger":";zulu", "expansion":"duplicate"})
        )
        .is_err());
        let mut invalid = rows[0].clone();
        invalid["file"] = json!("../outside.yml");
        assert!(
            handle(root.path(), json!({"action":"delete", "original":invalid}))
                .unwrap_err()
                .to_string()
                .contains("Invalid dictionary path")
        );
        for row in rows {
            handle(root.path(), json!({"action":"delete", "original":row}))?;
        }
        assert!(entries(root.path())?.is_empty());
        Ok(())
    }

    #[test]
    fn scalar_styles_preserve_neighbors() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("base.yml");
        for scalar in [
            "plain text # inline\n",
            "'single quoted' # inline\n",
            "\"double quoted\" # inline\n",
            "plain\n      continuation\n",
            ">- # header\n      folded\n      text\n",
            "|+\n      literal\n\n",
            "|2-\n      indented\n",
            "|\n",
        ] {
            fs::write(
                &path,
                format!(
                    "matches:\n  - trigger: ';x'\n    replace: {scalar}    vars: []\nother: true\n"
                ),
            )?;
            let original = entries(root.path())?[0].clone();
            handle(root.path(), json!({"action":"save", "original":original, "trigger":";y", "expansion":"new\ntext"})).with_context(|| format!("scalar {scalar:?}"))?;
            let saved = fs::read_to_string(&path)?;
            assert!(
                saved.ends_with("    vars: []\nother: true\n"),
                "{scalar:?}: {saved:?}"
            );
            if scalar.contains('#') {
                assert!(saved.contains(scalar.split('#').nth(1).unwrap().lines().next().unwrap()));
            }
        }
        Ok(())
    }

    #[test]
    fn rejects_unsafe_styles_without_writing() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("base.yml");
        for text in [
            "matches: [{trigger: ';x', replace: text}]\n",
            "base: &text hello\nmatches:\n  - trigger: ';x'\n    replace: *text\n",
            "matches:\n  - {trigger: ';x', replace: text}\n",
        ] {
            fs::write(&path, text)?;
            let original = entries(root.path())?[0].clone();
            assert!(handle(
                root.path(),
                json!({"action":"save", "original":original, "trigger":";y", "expansion":"new"})
            )
            .is_err());
            assert_eq!(fs::read_to_string(&path)?, text);
        }
        Ok(())
    }
}
