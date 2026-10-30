//! Bounded asynchronous dictionary lookups. No query text is persisted.
use crate::{config, discovery::Entry, util::*};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
type Row = (String, String, String);
pub fn encode(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}
pub fn plain(text: &str) -> String {
    let stripped = regex::Regex::new("<[^>]+>").unwrap().replace_all(text, "");
    let re = regex::Regex::new(r"&(#x[0-9a-fA-F]+|#[0-9]+|[A-Za-z][A-Za-z0-9]+);?").unwrap();
    re.replace_all(&stripped, |c: &regex::Captures| {
        let key = &c[1];
        if let Some(n) = key
            .strip_prefix("#x")
            .and_then(|s| u32::from_str_radix(s, 16).ok())
            .or_else(|| key.strip_prefix('#').and_then(|s| s.parse().ok()))
        {
            return char::from_u32(n).unwrap_or('\u{fffd}').to_string();
        }
        config::contract()["html_entities"]
            .get(format!("{key};"))
            .or_else(|| config::contract()["html_entities"].get(key))
            .and_then(Value::as_str)
            .unwrap_or(&c[0])
            .to_owned()
    })
    .trim()
    .to_owned()
}
fn array(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn fetch(url: &str, timeout: u64) -> Result<Value> {
    let out = run(
        &[
            "curl".into(),
            "-q".into(),
            "--silent".into(),
            "--show-error".into(),
            "--location".into(),
            "--proto".into(),
            "=https".into(),
            "--proto-redir".into(),
            "=https".into(),
            "--max-filesize".into(),
            "2000000".into(),
            "--max-time".into(),
            timeout.to_string(),
            "--user-agent".into(),
            "Runtjik/0.8 (dictionary lookup)".into(),
            "--header".into(),
            "Accept: application/json".into(),
            "--write-out".into(),
            "\n%{http_code}".into(),
            url.into(),
        ],
        None,
        timeout + 1,
    )?;
    let (body, status) = out
        .rsplit_once('\n')
        .ok_or_else(|| anyhow::anyhow!("Missing HTTP status"))?;
    if status == "404" {
        return Ok(json!([]));
    }
    if !status.starts_with('2') {
        bail!("HTTP {status}");
    }
    if body.len() > 2_000_000 {
        bail!("Response exceeds 2 MB");
    }
    Ok(serde_json::from_str(body)?)
}
pub fn parse(source: &str, command: &str, term: &str, data: &Value, limit: usize) -> Vec<Row> {
    let mut rows = vec![];
    let encoded = encode(term);
    match source {
        "free_dictionary" => {
            for entry in array(data) {
                let url = format!("https://en.wiktionary.org/wiki/{encoded}");
                for meaning in array(&entry["meanings"]) {
                    let part = s(&meaning["partOfSpeech"]);
                    if command == "/synonym" {
                        let mut words = strings(&meaning["synonyms"]);
                        for d in array(&meaning["definitions"]) {
                            words.extend(strings(&d["synonyms"]));
                        }
                        let mut seen = std::collections::HashSet::new();
                        for word in words {
                            if seen.insert(word.clone()) {
                                rows.push((word, part.into(), url.clone()));
                            }
                        }
                    } else {
                        for d in array(&meaning["definitions"]) {
                            if s(&d["definition"]).is_empty() {
                                continue;
                            }
                            let example = if s(&d["example"]).is_empty() {
                                String::new()
                            } else {
                                format!(" · Example: {}", s(&d["example"]))
                            };
                            rows.push((
                                s(&d["definition"]).into(),
                                format!("{part}{example}"),
                                url.clone(),
                            ));
                        }
                    }
                }
            }
        }
        "wiktionary" => {
            for meaning in array(&data["en"]) {
                for d in array(&meaning["definitions"]) {
                    let examples: Vec<_> = array(&d["examples"])
                        .iter()
                        .map(|s| plain(crate::util::s(s)))
                        .collect();
                    let suffix = if examples.is_empty() {
                        String::new()
                    } else {
                        format!(" · Examples: {}", examples.join(" / "))
                    };
                    rows.push((
                        plain(s(&d["definition"])),
                        format!("{}{suffix}", s(&meaning["partOfSpeech"])),
                        format!("https://en.wiktionary.org/wiki/{encoded}"),
                    ));
                }
            }
        }
        "urban_dictionary" => {
            for d in array(&data["list"]) {
                rows.push((
                    s(&d["definition"]).replace(['[', ']'], ""),
                    format!("Example: {}", s(&d["example"]).replace(['[', ']'], "")),
                    format!(
                        "https://www.urbandictionary.com/define.php?term={}&defid={}",
                        encoded,
                        d["defid"]
                            .as_i64()
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| s(&d["defid"]).into())
                    ),
                ));
            }
        }
        "datamuse" => {
            for d in array(data) {
                rows.push((
                    s(&d["word"]).into(),
                    format!("Synonym of {term}"),
                    format!("https://en.wiktionary.org/wiki/{}", encode(s(&d["word"]))),
                ));
            }
        }
        _ => {}
    }
    rows.truncate(limit);
    rows
}
fn lookup(source: &str, command: &str, term: &str, timeout: u64, limit: usize) -> Result<Vec<Row>> {
    let t = encode(term);
    let url = match source {
        "free_dictionary" => format!("https://api.dictionaryapi.dev/api/v2/entries/en/{t}"),
        "wiktionary" => format!("https://en.wiktionary.org/api/rest_v1/page/definition/{t}"),
        "urban_dictionary" => format!("https://api.urbandictionary.com/v0/define?term={t}"),
        "datamuse" => format!("https://api.datamuse.com/words?rel_syn={t}&max={limit}"),
        _ => bail!("Unknown word source"),
    };
    Ok(parse(source, command, term, &fetch(&url, timeout)?, limit))
}
struct Job {
    generation: u64,
    source: String,
    command: String,
    term: String,
    timeout: u64,
    limit: usize,
    reply: mpsc::Sender<(String, std::result::Result<Vec<Row>, String>)>,
}
pub struct Words {
    jobs: Option<mpsc::Sender<Job>>,
    generation: Arc<AtomicU64>,
    receiver: Option<mpsc::Receiver<(String, std::result::Result<Vec<Row>, String>)>>,
    key: String,
    sources: Vec<String>,
    results: std::collections::HashMap<String, std::result::Result<Vec<Row>, String>>,
    pub pending: bool,
}
impl Default for Words {
    fn default() -> Self {
        Self {
            jobs: None,
            generation: Arc::new(AtomicU64::new(0)),
            receiver: None,
            key: String::new(),
            sources: vec![],
            results: std::collections::HashMap::new(),
            pending: false,
        }
    }
}
impl Drop for Words {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
}
fn row(key: &str, name: &str, subtitle: &str, data: Value) -> Entry {
    let mut data = data;
    data["keep_open"] = json!(true);
    Entry {
        id: format!("Words:{key}"),
        name: name.into(),
        subtitle: subtitle.into(),
        icon: "accessories-dictionary".into(),
        provider: "Words".into(),
        keywords: String::new(),
        data,
    }
}
impl Words {
    fn pool(&mut self) {
        if self.jobs.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..3 {
            let rx = rx.clone();
            let generation = self.generation.clone();
            std::thread::spawn(move || loop {
                let Ok(job) = rx.lock().unwrap().recv() else {
                    break;
                };
                if job.generation != generation.load(Ordering::Relaxed) {
                    continue;
                }
                let result = lookup(&job.source, &job.command, &job.term, job.timeout, job.limit)
                    .map_err(|e| e.to_string());
                let _ = job.reply.send((job.source, result));
            });
        }
        self.jobs = Some(tx);
    }
    pub fn query(&mut self, q: &str, settings: &Value, demo: bool) -> Vec<Entry> {
        let (command, term) = q.trim().split_once(' ').unwrap_or((q.trim(), ""));
        let term = term.trim();
        if term.is_empty() {
            self.pending = false;
            return vec![row(
                "hint",
                "Type a word to look up",
                "Definitions and synonyms appear here as you type",
                json!({"disabled":true}),
            )];
        }
        if demo {
            self.pending = false;
            return vec![
                row(
                    "demo1",
                    if command == "/define" {
                        "To explain the meaning of a word or expression."
                    } else {
                        "explain"
                    },
                    "Synthetic dictionary · Verb · Example: Please define this term.",
                    json!({}),
                ),
                row(
                    "demo2",
                    if command == "/define" {
                        "To describe the limits or essential qualities of something."
                    } else {
                        "describe"
                    },
                    "Synthetic dictionary · Verb · Example: These principles define our work.",
                    json!({}),
                ),
            ];
        }
        let options = &settings["plugin_settings"]["Words"];
        let sources = strings(
            &options[if command == "/define" {
                "define_sources"
            } else {
                "synonym_sources"
            }],
        );
        let key = json!([command, term, sources, options]).to_string();
        if self.key != key {
            if !sources.is_empty() {
                self.pool();
            }
            self.key = key;
            self.sources = sources;
            self.results.clear();
            let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
            let (tx, rx) = mpsc::channel();
            self.receiver = Some(rx);
            for source in &self.sources {
                let _ = self.jobs.as_ref().unwrap().send(Job {
                    generation,
                    source: source.clone(),
                    command: command.into(),
                    term: term.into(),
                    timeout: options["timeout_seconds"].as_u64().unwrap(),
                    limit: options["max_results_per_source"].as_u64().unwrap() as usize,
                    reply: tx.clone(),
                });
            }
        }
        if let Some(rx) = &self.receiver {
            while let Ok((source, result)) = rx.try_recv() {
                self.results.insert(source, result);
            }
        }
        self.pending = self.results.len() < self.sources.len();
        let mut rows = vec![];
        for source in &self.sources {
            if let Some(result) = self.results.get(source) {
                match result {
                    Ok(values) if values.is_empty() => rows.push(row(
                        &format!("{source}:empty"),
                        "No entries found",
                        &source.replace('_', " "),
                        json!({"disabled":true}),
                    )),
                    Ok(values) => {
                        for (i, (name, example, url)) in values.iter().enumerate() {
                            rows.push(row(
                                &format!("{source}:{i}"),
                                name,
                                &format!("{} · {example}", source.replace('_', " ")),
                                json!({"url":url}),
                            ))
                        }
                    }
                    Err(e) => rows.push(row(
                        &format!("{source}:error"),
                        "Source unavailable",
                        &format!("{source}: {e}"),
                        json!({"disabled":true}),
                    )),
                }
            }
        }
        if self.pending {
            rows.push(row(
                "loading",
                &format!("Looking up “{term}”…"),
                "Results appear as each source responds",
                json!({"disabled":true}),
            ));
        } else if rows.is_empty() {
            rows.push(row(
                "empty",
                "No sources enabled",
                "Edit /config plugin_settings.Words",
                json!({"disabled":true}),
            ));
        }
        rows
    }
    pub fn activate(entry: &Entry, demo: bool) -> Result<Value> {
        if let Some(source) = entry.data["url"].as_str() {
            let url = url::Url::parse(source)?;
            if url.scheme() != "https"
                || ![Some("en.wiktionary.org"), Some("www.urbandictionary.com")]
                    .contains(&url.host_str())
            {
                bail!("Invalid word source URL");
            }
            if !demo {
                dispatch(
                    vec!["xdg-open".into(), source.into()],
                    &json!({"launch":{"isolate":false}}),
                )?;
            }
            return Ok(json!({"kind":"message","message":"Opened source · results stay here"}));
        }
        Ok(json!({"kind":"message","message":"Results update as you type"}))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_examples_and_synonym_order() {
        let data = json!([{"meanings":[{"partOfSpeech":"verb","synonyms":["explain"],"definitions":[{"definition":"Describe","example":"An example","synonyms":["explain","describe"]}]}]}]);
        assert_eq!(
            parse("free_dictionary", "/define", "test", &data, 10)[0].1,
            "verb · Example: An example"
        );
        assert_eq!(
            parse("free_dictionary", "/synonym", "test", &data, 10)
                .iter()
                .map(|r| r.0.as_str())
                .collect::<Vec<_>>(),
            ["explain", "describe"]
        );
        assert_eq!(plain(" <b>A &amp; B</b> &#x1F41F; &eacute; "), "A & B 🐟 é");
    }
    #[test]
    fn no_sources_and_demo_do_not_start_workers() {
        let mut w = Words::default();
        let mut settings = config::contract()["DEFAULTS"].clone();
        settings["plugin_settings"]["Words"]["define_sources"] = json!([]);
        assert_eq!(
            w.query("/define test", &settings, false)[0].id,
            "Words:empty"
        );
        assert!(!w.pending);
    }
}
