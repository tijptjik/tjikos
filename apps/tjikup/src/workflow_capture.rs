//! Capture live preferences while preserving template expressions and controls.
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    name: String,
    source: PathBuf,
    target: PathBuf,
    kind: String,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    backup: bool,
    #[serde(default)]
    field: String,
    #[serde(default)]
    short_hostname: bool,
}
fn actions(source: &str) -> Result<Vec<(usize, usize)>> {
    let mut result = Vec::new();
    let mut at = 0;
    while at < source.len() {
        let expr = source[at..].find("{{").map(|p| p + at);
        let control = source[at..].find("{%").map(|p| p + at);
        let Some(start) = expr.into_iter().chain(control).min() else {
            break;
        };
        let close = if &source[start..start + 2] == "{{" {
            "}}"
        } else {
            "%}"
        };
        let end = source[start + 2..]
            .find(close)
            .context("unterminated template action")?
            + start
            + 4;
        result.push((start, end));
        at = end;
    }
    Ok(result)
}
fn comments(source: &str) -> Result<Vec<u8>> {
    let mut bytes = source.as_bytes().to_vec();
    let mut at = 0;
    let mut quoted = false;
    let mut escape = false;
    while at < bytes.len() {
        let ch = bytes[at];
        if quoted {
            if escape {
                escape = false;
            } else if ch == b'\\' {
                escape = true;
            } else if ch == b'"' {
                quoted = false;
            }
            at += 1;
            continue;
        }
        if ch == b'"' {
            quoted = true;
            at += 1;
            continue;
        }
        if bytes.get(at..at + 2) == Some(b"//") {
            while at < bytes.len() && bytes[at] != b'\n' {
                bytes[at] = b' ';
                at += 1;
            }
            continue;
        }
        if bytes.get(at..at + 2) == Some(b"/*") {
            bytes[at] = b' ';
            bytes[at + 1] = b' ';
            at += 2;
            while at + 1 < bytes.len() && &bytes[at..at + 2] != b"*/" {
                if bytes[at] != b'\n' {
                    bytes[at] = b' ';
                }
                at += 1;
            }
            if at + 1 >= bytes.len() {
                bail!("unterminated JSONC comment");
            }
            bytes[at] = b' ';
            bytes[at + 1] = b' ';
            at += 2;
            continue;
        }
        at += 1;
    }
    Ok(bytes)
}
struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
    spans: BTreeMap<Vec<String>, (usize, usize)>,
}
impl Parser<'_> {
    fn space(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }
    fn string(&mut self) -> Result<String> {
        let start = self.at;
        if self.bytes.get(self.at) != Some(&b'"') {
            bail!("expected JSON string");
        }
        self.at += 1;
        let mut escaped = false;
        while let Some(&b) = self.bytes.get(self.at) {
            self.at += 1;
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                return Ok(serde_json::from_slice(&self.bytes[start..self.at])?);
            }
        }
        bail!("unterminated JSON string")
    }
    fn value(&mut self, path: Vec<String>) -> Result<()> {
        self.space();
        let start = self.at;
        match self.bytes.get(self.at).context("unexpected end of JSON")? {
            b'{' | b'[' => {
                let object = self.bytes[self.at] == b'{';
                let end = if object { b'}' } else { b']' };
                self.at += 1;
                self.space();
                let mut index = 0;
                while self.bytes.get(self.at) != Some(&end) {
                    self.space();
                    let key = if object {
                        let k = self.string()?;
                        self.space();
                        if self.bytes.get(self.at) != Some(&b':') {
                            bail!("missing colon");
                        }
                        self.at += 1;
                        k
                    } else {
                        index.to_string()
                    };
                    let mut nested = path.clone();
                    nested.push(key);
                    self.value(nested)?;
                    index += 1;
                    self.space();
                    if self.bytes.get(self.at) == Some(&end) {
                        break;
                    }
                    if self.bytes.get(self.at) != Some(&b',') {
                        bail!("missing comma");
                    }
                    self.at += 1;
                    self.space();
                }
                self.at += 1;
            }
            b'"' => {
                self.string()?;
            }
            _ => {
                while self
                    .bytes
                    .get(self.at)
                    .is_some_and(|b| !b.is_ascii_whitespace() && !b",]}".contains(b))
                {
                    self.at += 1;
                }
                // An unquoted template expression is masked to whitespace.
                // The following delimiter is consumed by the containing object.
                if start == self.at && !self.bytes.get(self.at).is_some_and(|b| b",]}".contains(b))
                {
                    bail!("invalid JSON scalar");
                }
            }
        }
        self.spans.insert(path, (start, self.at));
        Ok(())
    }
}
fn scalar_capture(source: &str, target: &str) -> Result<String> {
    let target: Value = serde_json::from_slice(&comments(target)?)?;
    let mut masked = comments(source)?;
    let mut stack = Vec::new();
    let mut protected = Vec::new();
    for (start, end) in actions(source)? {
        let body = source[start + 2..end - 2].trim_matches(|c: char| c.is_whitespace() || c == '-');
        let word = body.split_whitespace().next().unwrap_or_default();
        if ["if", "with", "range", "for"].contains(&word) {
            stack.push(start);
        } else if ["end", "endif", "endfor", "endwith"].contains(&word) {
            if let Some(begin) = stack.pop() {
                protected.push((begin, end));
            } else {
                bail!("unbalanced template controls");
            }
        }
        for b in &mut masked[start..end] {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    }
    if !stack.is_empty() {
        bail!("unbalanced template controls");
    }
    let mut parser = Parser {
        bytes: &masked,
        at: 0,
        spans: BTreeMap::new(),
    };
    parser.value(Vec::new())?;
    parser.space();
    if parser.at != masked.len() {
        bail!("unparsed template JSON");
    }
    let mut replacements = Vec::new();
    for (path, (start, end)) in parser.spans {
        if path.is_empty() || protected.iter().any(|(a, b)| *a <= start && end <= *b) {
            continue;
        }
        let mut live = &target;
        let mut missing = false;
        for part in &path {
            live = match live {
                Value::Object(m) => match m.get(part) {
                    Some(v) => v,
                    None => {
                        missing = true;
                        break;
                    }
                },
                Value::Array(a) => match part.parse::<usize>().ok().and_then(|i| a.get(i)) {
                    Some(v) => v,
                    None => {
                        missing = true;
                        break;
                    }
                },
                _ => bail!("structural changes require manual template merge"),
            };
        }
        if missing || source[start..end].contains("{{") || live.is_array() || live.is_object() {
            continue;
        }
        let Ok(current) = serde_json::from_slice::<Value>(&masked[start..end]) else {
            continue;
        };
        if &current != live {
            replacements.push((start, end, serde_json::to_string(live)?));
        }
    }
    replacements.sort_by_key(|a| std::cmp::Reverse(a.0));
    let mut updated = source.to_owned();
    for (start, end, value) in replacements {
        updated.replace_range(start..end, &value);
    }
    Ok(updated)
}
fn preserve(value: &Value, source: &Value, home: &str, expression: &str) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), preserve(v, &source[k], home, expression)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| preserve(v, &source[i], home, expression))
                .collect(),
        ),
        _ => {
            let home_path = value
                .as_str()
                .filter(|v| *v == home || v.starts_with(&format!("{home}/")));
            if let Some(original) = source.as_str().filter(|s| s.contains("{{")) {
                if original.contains("user.home") {
                    if let Some(path) = home_path {
                        return json!(format!("{expression}{}", &path[home.len()..]));
                    }
                }
                return json!(original);
            }
            if let Some(v) = home_path {
                return json!(format!("{expression}{}", &v[home.len()..]));
            }
            value.clone()
        }
    }
}
fn transform(rule: &Rule, source: &str, target: &str, home: &str, host: &str) -> Result<String> {
    match rule.kind.as_str() {
        "copy" => Ok(target.into()),
        "jsonc-scalars" => scalar_capture(source, target),
        "host-text" => {
            let mut data: Value = serde_json::from_str(source)?;
            let map = data[&rule.field]
                .as_object_mut()
                .context("capture field must map hostnames to text")?;
            if map.values().any(|v| !v.is_string()) {
                bail!("capture field contains non-text values");
            }
            map.insert(
                if rule.short_hostname {
                    host.split('.').next().unwrap()
                } else {
                    host
                }
                .into(),
                json!(target),
            );
            Ok(serde_json::to_string_pretty(&data)? + "\n")
        }
        "template-json" => {
            let source_value: Value = serde_json::from_str(source)?;
            let live: Value = serde_json::from_str(target)?;
            let mut expression = "{{ user.home }}";
            for (start, end) in actions(source)? {
                let action = &source[start..end];
                let word = action[2..action.len() - 2]
                    .split_whitespace()
                    .next()
                    .unwrap_or_default();
                if action.starts_with("{%") {
                    bail!("unsupported capture control template");
                }
                if [
                    "if", "elif", "else", "endif", "end", "range", "for", "endfor", "with",
                    "endwith",
                ]
                .contains(&word)
                {
                    bail!("unsupported capture control template");
                }
                if word == "user.home" {
                    expression = action;
                }
            }
            let updated = preserve(&live, &source_value, home, expression);
            if updated == source_value {
                return Ok(source.to_owned());
            }
            let mut text = serde_json::to_string_pretty(&updated)?;
            if target.ends_with('\n') {
                text.push('\n');
            }
            Ok(text)
        }
        _ => bail!("unknown capture kind"),
    }
}
pub fn run(repo: &Path, args: &[String]) -> Result<i32> {
    if args.iter().any(|s| s != "--dry-run") {
        bail!("capture supports --dry-run");
    }
    let dry = !args.is_empty();
    let rules: Vec<Rule> = serde_json::from_slice(&fs::read(repo.join("tjikup/capture.json"))?)?;
    let home = std::env::var("HOME")?;
    let host = crate::device::hostname()?;
    let mut pending = Vec::new();
    for rule in rules {
        if rule.source.is_absolute()
            || rule.target.is_absolute()
            || rule
                .source
                .components()
                .chain(rule.target.components())
                .any(|c| c == std::path::Component::ParentDir)
        {
            bail!("capture paths must stay within their roots");
        }
        let source = repo.join(&rule.source);
        let target = Path::new(&home).join(&rule.target);
        let before = fs::read_to_string(&source)
            .with_context(|| format!("{}: missing capture source", rule.name))?;
        if !target.is_file() && rule.optional {
            println!("SKIP    - {}  not installed", rule.name);
            continue;
        }
        let target = fs::read_to_string(target)?;
        let after = transform(&rule, &before, &target, &home, &host)
            .with_context(|| format!("{} capture failed", rule.name))?;
        pending.push((rule, source, before, after));
    }
    // Prepare every transformation before any source update. Back up requested live captures first.
    if !dry {
        for (rule, _, _, after) in &pending {
            if rule.backup {
                let state = PathBuf::from(
                    std::env::var_os("XDG_STATE_HOME")
                        .unwrap_or_else(|| Path::new(&home).join(".local/state").into()),
                )
                .join("tjikup/backups")
                .join(&rule.name);
                if rule.name.contains('/') || rule.name == ".." {
                    bail!("invalid capture name");
                }
                fs::create_dir_all(&state)?;
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&state, fs::Permissions::from_mode(0o700))?;
                let mut file = tempfile::Builder::new()
                    .prefix("capture-")
                    .suffix(".backup")
                    .tempfile_in(state)?;
                file.write_all(after.as_bytes())?;
                file.as_file().sync_all()?;
                file.keep()?;
            }
        }
    }
    for (rule, path, before, after) in pending {
        if before == after {
            println!("SKIP    ✓ {}  no changes", rule.name);
            continue;
        }
        if !dry {
            if fs::read_to_string(&path)? != before {
                bail!("capture source changed concurrently; retry");
            }
            let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
            file.as_file()
                .set_permissions(fs::metadata(&path)?.permissions())?;
            file.write_all(after.as_bytes())?;
            file.as_file().sync_all()?;
            file.persist(path)?;
        }
        println!(
            "SYNC    ✓ {}  {}",
            rule.name,
            if dry {
                "would capture changes"
            } else {
                "captured changes"
            }
        );
    }
    Ok(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scalar_capture_preserves_comments_and_expressions() {
        let source = "{\n// keep\n\"size\": 12, \"home\": \"{{ user.home }}\"\n}";
        let result = scalar_capture(source, r#"{"size":14,"home":"/home/test"}"#).unwrap();
        assert!(result.contains("// keep"));
        assert!(result.contains("\"size\": 14"));
        assert!(result.contains("{{ user.home }}"));
    }
    #[test]
    fn structural_changes_refused() {
        assert!(scalar_capture(r#"{"x":{"y":1}}"#, r#"{"x":2}"#).is_err());
    }
    #[test]
    fn controls_protected() {
        let s = "{\n{% if is_client %}\"size\":12,{% endif %}\n\"other\":1}";
        let result = scalar_capture(s, r#"{"size":14,"other":2}"#).unwrap();
        assert!(result.contains("\"size\":12"));
        assert!(result.contains("\"other\":2"));
    }
}
