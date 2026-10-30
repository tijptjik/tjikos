use crate::{config::contract, util::*};
use anyhow::Result;
use gio::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default = "default_icon")]
    pub icon: String,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub keywords: String,
    #[serde(default = "empty_data")]
    pub data: Value,
}
fn default_icon() -> String {
    "application-x-executable".into()
}
fn default_provider() -> String {
    "apps".into()
}
fn empty_data() -> Value {
    json!({})
}
fn paths() -> Vec<PathBuf> {
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .filter(|p| p.is_absolute())
        .collect()
}
fn app_dirs() -> Vec<PathBuf> {
    let mut roots = vec![env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/share"))];
    roots.extend(env::split_paths(
        &env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
    ));
    roots.extend([
        home().join(".local/share/flatpak/exports/share"),
        PathBuf::from("/var/lib/flatpak/exports/share"),
        PathBuf::from("/var/lib/snapd/desktop"),
    ]);
    let mut seen = HashSet::new();
    roots
        .into_iter()
        .map(|p| p.join("applications"))
        .filter(|p| seen.insert(p.clone()))
        .collect()
}
fn files(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut v: Vec<_> = walkdir::WalkDir::new(dir)
        .follow_links(true)
        .max_depth(if recursive { usize::MAX } else { 1 })
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.depth() > 0 && (!recursive || e.path().extension().is_some_and(|s| s == "desktop"))
        })
        .map(|e| e.into_path())
        .collect();
    v.sort();
    v
}
fn history_path() -> Option<PathBuf> {
    let n = env::var("fish_history").unwrap_or("fish".into());
    if n.is_empty() || !n.chars().all(|c| c.is_alphanumeric() || "_.-".contains(c)) {
        return None;
    }
    Some(
        xdg("data")
            .parent()?
            .join("fish")
            .join(format!("{n}_history")),
    )
}
fn signature(settings: &Value) -> String {
    let mut hash = Sha256::new();
    hash.update(settings["index"].to_string());
    for key in [
        "PATH",
        "XDG_CURRENT_DESKTOP",
        "LANG",
        "LC_ALL",
        "LC_MESSAGES",
    ] {
        hash.update(env::var(key).unwrap_or_default());
    }
    for (dirs, recursive) in [
        (
            if settings["index"]["desktop_apps"] == true {
                app_dirs()
            } else {
                vec![]
            },
            true,
        ),
        (
            if settings["index"]["path_commands"] == true {
                paths()
            } else {
                vec![]
            },
            false,
        ),
    ] {
        for dir in dirs {
            let mut v = vec![dir.clone()];
            v.extend(files(&dir, recursive));
            for p in v {
                hash.update(p.to_string_lossy().as_bytes());
                if let Ok(m) = p.metadata() {
                    hash.update(format!(
                        "{}:{}:{}:{}:{}",
                        m.mtime(),
                        m.mtime_nsec(),
                        m.mode(),
                        m.len(),
                        m.ino()
                    ));
                }
            }
        }
    }
    if settings["index"]["path_commands"] == true {
        if let Some(p) = history_path() {
            hash.update(p.to_string_lossy().as_bytes());
            if let Ok(m) = p.metadata() {
                hash.update(format!(
                    "{}:{}:{}:{}",
                    m.mtime(),
                    m.mtime_nsec(),
                    m.len(),
                    m.ino()
                ));
            }
        }
    }
    format!("{:x}", hash.finalize())
}
fn contains(c: &Value, key: &str, v: &str) -> bool {
    c[key].as_array().unwrap().iter().any(|x| x == v)
}
fn basename(p: &str) -> String {
    Path::new(p)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into()
}
fn apps(settings: &Value) -> Vec<Entry> {
    if settings["index"]["desktop_apps"] != true {
        return vec![];
    }
    let c = contract();
    let mut seen = HashSet::new();
    let mut out = vec![];
    for dir in app_dirs() {
        for path in files(&dir, true) {
            let id = path
                .strip_prefix(&dir)
                .unwrap()
                .to_string_lossy()
                .replace('/', "-");
            if !seen.insert(id.clone()) || contains(&c, "HIDDEN_APPS", &id) {
                continue;
            }
            let Some(a) = gio::DesktopAppInfo::from_filename(&path) else {
                continue;
            };
            if a.is_hidden() || !a.should_show() {
                continue;
            }
            let executable = a.executable().to_string_lossy().to_string();
            if executable.is_empty() && !a.boolean("DBusActivatable")
                || !executable.is_empty() && which(&executable).is_none()
            {
                continue;
            }
            let name = c["APP_NAMES"][&id]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| a.display_name().into());
            if ["hamr", "hamr launcher"].contains(&name.to_lowercase().as_str())
                || contains(&c, "HIDDEN_COMMANDS", &basename(&executable))
                || ["hamr.desktop", "hamr-gtk.desktop", "com.hamr.hamr.desktop"]
                    .contains(&id.to_lowercase().as_str())
            {
                continue;
            }
            let mut classes = vec![];
            if let Some(wm) = a.startup_wm_class() {
                classes.push(wm.to_string());
            }
            classes.push(id.trim_end_matches(".desktop").into());
            if a.startup_wm_class().is_none() {
                classes.push(basename(&executable));
            }
            classes.dedup();
            let mut aliases = vec![format!("apps:{id}")];
            aliases.extend(
                strings(&c["HISTORY_ALIASES"][&id])
                    .iter()
                    .map(|v| format!("apps:{v}")),
            );
            out.push(Entry{id:format!("apps:{id}"),name,subtitle:a.generic_name().map(|s|s.to_string()).unwrap_or("Application".into()),icon:c["APP_ICONS"][&id].as_str().map(str::to_owned).or_else(||a.icon().and_then(|i|i.to_string()).map(|s|s.to_string())).unwrap_or_else(default_icon),provider:"apps".into(),keywords:format!("{} {}",a.keywords().iter().map(|s|s.as_str()).collect::<Vec<_>>().join(" "),a.description().unwrap_or_default()),data:json!({"path":path,"desktop_id":id,"executable":executable,"commandline":a.commandline().map(|s|s.to_string_lossy().to_string()).unwrap_or_default(),"terminal":a.boolean("Terminal"),"classes":classes,"aliases":aliases})});
        }
    }
    out
}
fn literal(v: &str) -> Option<String> {
    if v.contains(['$', '(', ')', '{', '}', '*', '?', '\n']) {
        return None;
    }
    let v = shlex::split(v)?;
    if v.len() == 1 {
        v.into_iter().next()
    } else {
        None
    }
}
fn history_head(words: &[Option<String>]) -> Option<String> {
    let mut i = 0;
    while i < words.len() {
        let name = words[i].as_deref()?;
        i += 1;
        if !["sudo", "doas", "env", "nohup", "nice", "mullvad-exclude"].contains(&name) {
            return if name.starts_with('-') {
                None
            } else {
                Some(basename(name))
            };
        }
        while i < words.len() {
            let w = words[i].as_deref()?;
            if w == "--" {
                i += 1;
                break;
            }
            if name == "env"
                && regex::Regex::new("^[A-Za-z_][A-Za-z0-9_]*=")
                    .unwrap()
                    .is_match(w)
            {
                i += 1;
                continue;
            }
            let opts = match name {
                "sudo" => vec![
                    "-u", "-g", "-h", "-p", "-C", "-T", "-R", "-D", "--user", "--group", "--host",
                    "--prompt", "--chdir", "--chroot",
                ],
                "doas" => vec!["-u", "-C"],
                "env" => vec!["-u", "--unset", "-C", "--chdir"],
                "nice" => vec!["-n", "--adjustment"],
                _ => vec![],
            };
            if opts.contains(&w) {
                i += 2;
            } else if w.starts_with('-') {
                if ["-s", "-i", "-S", "--split-string", "--shell", "--login"].contains(&w) {
                    return None;
                }
                i += 1;
            } else {
                break;
            }
        }
    }
    None
}
fn fish_commands() -> HashSet<String> {
    let mut result = HashSet::new();
    let Some(path) = history_path() else {
        return result;
    };
    let Ok(text) = fs::read_to_string(path) else {
        return result;
    };
    let mut seen = HashSet::new();
    let records: Vec<_> = text
        .lines()
        .filter_map(|l| l.strip_prefix("- cmd: "))
        .map(|l| {
            let mut s = String::new();
            let mut chars = l.chars();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    match chars.next() {
                        Some('n') => s.push('\n'),
                        Some('\\') => s.push('\\'),
                        Some(c) => {
                            s.push('\\');
                            s.push(c)
                        }
                        None => s.push('\\'),
                    }
                } else {
                    s.push(c)
                }
            }
            s
        })
        .filter(|s| seen.insert(s.clone()))
        .collect();
    if records.is_empty() {
        return result;
    }
    // Fish prints its syntax tree to stderr, while its formatted program is discarded.
    let script = "exec fish_indent --dump-parse-tree 2>&1 >/dev/null";
    let Ok(tree) = run(
        &["sh".into(), "-c".into(), script.into()],
        Some(&(records.join("\n") + "\n")),
        8,
    ) else {
        return result;
    };
    let mut level = None;
    let mut words = vec![];
    let finish = |words: &Vec<Option<String>>, result: &mut HashSet<String>| {
        if let Some(Some(w)) = words.first() {
            result.insert(basename(w));
        }
        if let Some(w) = history_head(words) {
            result.insert(w);
        }
    };
    for line in tree.lines() {
        let node = line.trim_start_matches(['!', ' ']);
        let depth = line.len() - node.len();
        if level.is_some_and(|n| depth <= n) {
            finish(&words, &mut result);
            words.clear();
            level = None;
        }
        if node == "decorated_statement" {
            level = Some(depth);
        } else if level.is_some()
            && (node.starts_with("string: '") || node.starts_with("argument: '"))
            && node.ends_with('\'')
        {
            let val = node.split_once(": ").unwrap().1;
            words.push(literal(&val[1..val.len() - 1]));
        }
    }
    finish(&words, &mut result);
    result
}
fn decorate(entries: &mut [Entry]) {
    let c = contract();
    let mut applications: HashMap<String, Vec<Entry>> = HashMap::new();
    for e in entries.iter().filter(|e| e.provider == "apps") {
        let ex = s(&e.data["executable"]);
        let args = shlex::split(s(&e.data["commandline"])).unwrap_or_default();
        if ex.is_empty()
            || contains(&c, "GUI_WRAPPERS", &basename(ex))
            || e.data["terminal"] == true
            || args.iter().skip(1).any(|a| !a.starts_with('%'))
        {
            continue;
        }
        for k in [
            Some(basename(ex)),
            which(ex)
                .and_then(|p| p.canonicalize().ok())
                .map(|p| p.to_string_lossy().to_string()),
        ]
        .into_iter()
        .flatten()
        {
            applications.entry(k).or_default().push(e.clone());
        }
    }
    for e in entries.iter_mut().filter(|e| e.provider == "commands") {
        e.icon = "org.gnome.Terminal".into();
        let argv = strings(&e.data["argv"]);
        let key = argv
            .first()
            .and_then(|p| Path::new(p).canonicalize().ok())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let matches = applications.get(&key).or_else(|| applications.get(&e.name));
        if let Some(a) = matches {
            if a.len() == 1 && !contains(&c, "GUI_WRAPPERS", &e.name) {
                e.icon = a[0].icon.clone();
                e.data["gui"] = json!(true);
                e.data["classes"] = a[0].data["classes"].clone();
            }
        }
        if let Some(icon) = c["COMMAND_ICONS"][&e.name].as_str() {
            e.icon = icon.into();
            if contains(&c, "GUI_COMMANDS", &e.name) {
                e.data["gui"] = json!(true);
            }
        }
    }
}
pub fn discover(settings: &Value, force: bool) -> Result<Vec<Entry>> {
    let sig = signature(settings);
    let path = xdg("cache").join("index-rust-v1.json");
    if !force {
        if let Ok(v) = fs::read(&path) {
            if let Ok(v) = serde_json::from_slice::<Value>(&v) {
                if v["signature"] == sig {
                    if let Ok(e) = serde_json::from_value(v["entries"].clone()) {
                        return Ok(e);
                    }
                }
            }
        }
    }
    let mut entries = apps(settings);
    if settings["index"]["path_commands"] == true {
        let allowed = fish_commands();
        let c = contract();
        let mut seen = HashSet::new();
        for dir in paths() {
            for p in files(&dir, false) {
                let name = basename(&p.to_string_lossy());
                if allowed.contains(&name)
                    && !contains(&c, "HIDDEN_COMMANDS", &name)
                    && which(&p.to_string_lossy()).is_some()
                    && seen.insert(name.clone())
                {
                    entries.push(Entry {
                        id: format!("commands:{name}"),
                        name,
                        subtitle: dir.to_string_lossy().to_string(),
                        icon: "org.gnome.Terminal".into(),
                        provider: "commands".into(),
                        keywords: String::new(),
                        data: json!({"argv":[p]}),
                    });
                }
            }
        }
    }
    decorate(&mut entries);
    atomic(
        &path,
        &serde_json::to_vec(&json!({"signature":sig,"entries":entries}))?,
    )?;
    Ok(entries)
}
pub fn folded(text: &str) -> String {
    let table = &contract()["casefold"];
    let mut output = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            output.push(c.to_ascii_lowercase());
        } else if let Some(mapped) = table.get(c.to_string()).and_then(Value::as_str) {
            output.push_str(mapped);
        } else {
            output.extend(c.to_lowercase());
        }
    }
    output
}
pub fn score(query: &str, e: &Entry) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let name = folded(&e.name);
    let hay = format!("{} {} {}", name, folded(&e.keywords), folded(&e.subtitle));
    let mut score = 0;
    for w in folded(query).split_whitespace() {
        score += if w == name {
            1000
        } else if name.starts_with(w) {
            500
        } else if name.contains(w) {
            250
        } else if hay.contains(w) {
            80
        } else {
            let mut chars = name.chars();
            if !w.chars().all(|c| chars.any(|x| x == c)) {
                return None;
            }
            20
        };
    }
    Some(score)
}
