use crate::{
    config,
    discovery::{self, Entry},
    icons::Icons,
    store::Store,
    util::*,
};
use anyhow::{bail, Context, Result};
use gio::prelude::FileExt;
use serde_json::{json, Value};
use std::{collections::HashMap, time::Instant};
pub struct Launcher {
    pub entries: Vec<Entry>,
    settings: Value,
    demo: bool,
    store: Option<Store>,
    icons: Icons,
    warnings: Vec<String>,
    checked: Instant,
    current: HashMap<String, Entry>,
    ranking: Option<HashMap<String, f64>>,
    orders: HashMap<String, Vec<String>>,
    last_query: String,
    page_query: String,
    page: Vec<(Entry, i64)>,
    closing: crate::close::Tracking,
    words: crate::words::Words,
    browser: crate::browser::Browser,
    todo: crate::todo::Todo,
    reading: crate::reading::Reading,
    clipboard: crate::clipboard::Clipboard,
    captures: crate::capture::Captures,
    file_root: std::path::PathBuf,
    file_search: crate::file_search::FileSearch,
    monitor: crate::monitor::Monitor,
    notes: crate::note::Notes,
}
impl Launcher {
    pub fn new(demo: bool) -> Result<Self> {
        let settings = config::load()?;
        let icons = Icons::new(&settings);
        let store = if demo { None } else { Some(Store::open()?) };
        let configured_root = s(&settings["file_search"]["root"]);
        let configured_root = if configured_root.is_empty() {
            home()
        } else if configured_root == "~" {
            home()
        } else if let Some(tail) = configured_root.strip_prefix("~/") {
            home().join(tail)
        } else {
            std::path::PathBuf::from(configured_root)
        };
        let file_root = configured_root
            .is_dir()
            .then(|| configured_root.canonicalize().ok())
            .flatten()
            .unwrap_or_else(home);
        let mut app = Self {
            entries: vec![],
            settings,
            demo,
            store,
            icons,
            warnings: vec![],
            checked: Instant::now(),
            current: HashMap::new(),
            ranking: None,
            orders: HashMap::new(),
            last_query: String::new(),
            page_query: String::new(),
            page: vec![],
            closing: crate::close::Tracking::new(),
            words: crate::words::Words::default(),
            browser: crate::browser::Browser::default(),
            todo: crate::todo::Todo::default(),
            reading: crate::reading::Reading::default(),
            clipboard: crate::clipboard::Clipboard::default(),
            captures: crate::capture::Captures::default(),
            file_root,
            file_search: crate::file_search::FileSearch::default(),
            monitor: crate::monitor::Monitor::default(),
            notes: crate::note::Notes::default(),
        };
        app.refresh(false)?;
        Ok(app)
    }
    pub fn refresh(&mut self, force: bool) -> Result<()> {
        self.entries = if self.demo {
            serde_json::from_value(config::contract()["demo_entries"].clone())?
        } else {
            discovery::discover(&self.settings, force)?
        };
        if force {
            self.icons = Icons::new(&self.settings);
        }
        self.checked = Instant::now();
        Ok(())
    }
    fn reset(&mut self) {
        self.ranking = None;
        self.orders.clear();
        self.page.clear();
        self.page_query.clear();
        self.last_query.clear();
    }
    fn history(&self) -> Result<Value> {
        let mut history = if self.settings["history"]["enabled"] == true {
            self.store
                .as_ref()
                .map(|s| s.history())
                .transpose()?
                .unwrap_or_default()
        } else {
            vec![]
        };
        history.retain(|query| !query.trim_start().starts_with("/bw ") && query.trim() != "/bw");
        Ok(json!(history))
    }
    fn tracking_enabled(&self) -> bool {
        self.settings["history"]["enabled"] == true && self.settings["stats"]["enabled"] == true
    }
    pub fn initial(&mut self) -> Result<Value> {
        if self.tracking_enabled() {
            if let Some(store) = &self.store {
                store.record("launcher", "open", true)?;
            }
        }
        let mut v = self.query("", 0, None)?;
        v["command_history"] = self.history()?;
        Ok(v)
    }
    fn provider_error(&self, name: &str, display: &str, error: anyhow::Error) -> Value {
        let mut v = self.payload(vec![]);
        v["query"] = json!(display);
        v["message"] = json!(format!("{name}: {error}"));
        v["app_mode"] = json!(if name == "Todo" { "Todo" } else { "" });
        v
    }
    fn provider_payload(
        &mut self,
        entries: Vec<Entry>,
        display: &str,
        offset: usize,
        limit: usize,
        pending: bool,
        browser: bool,
    ) -> Value {
        let total = entries.len();
        self.current = entries.iter().map(|e| (e.id.clone(), e.clone())).collect();
        let rows:Vec<_>=entries.into_iter().skip(offset).take(limit).map(|e| { let mut row = json!({"id":e.id,"name":e.name,"subtitle":e.subtitle,"provider":e.provider,"icon":self.icons.resolve(&e.icon),"favicon":s(&e.data["favicon"]),"hide_launcher":e.data["hide_launcher"]==true || e.data["capture"].as_str().is_some_and(|s|!s.is_empty()),"keep_open":e.data["keep_open"]==true,"thumbnail":s(&e.data["thumbnail"]),"empty_state":e.data["empty_state"]==true,"disabled":e.data["disabled"]==true,"count":0,"windows":0});
            if e.provider == "Emoji" {
                row["preview"] = e.data["emoji"].clone();
                row["has_tones"] = e.data["has_tones"].clone();
            }
            for key in ["recent_word", "lookup_query", "word_source", "part_of_speech", "examples", "url", "language", "section", "group", "etymology", "headword", "sense_number", "sense_depth", "sense_synonyms", "terms", "context"] {
                if e.provider == "Words" {
                    if let Some(value) = e.data.get(key) { row[key] = value.clone(); }
                }
            }
            row
        }).collect();
        let next = offset + rows.len();
        let mut v = self.payload(rows);
        let scope = if browser {
            self.browser.scope.as_ref()
        } else {
            None
        };
        for(k,val)in json!({"query":display,"browser_tab_scope":scope.is_some(),"message":scope.map(|(_,_,title)|format!("Tabs in {title} · Esc returns to search")).unwrap_or_default(),"pending":pending,"total":total,"offset":offset,"next_offset":next,"has_more":next<total}).as_object().unwrap(){v[k]=val.clone();}
        let _ = self.icons.flush();
        v
    }
    fn payload(&self, rows: Vec<Value>) -> Value {
        json!({"kind":"results","version":"0.8.3","results":rows,"config":self.settings,"indexed":self.entries.len(),"warnings":self.warnings,"demo":self.demo})
    }
    fn clients(&mut self) -> Vec<Value> {
        if self.demo {
            return config::contract()["demo_windows"]
                .as_array()
                .unwrap()
                .clone();
        }
        if !tjik_desktop::selected().diagnostics().capabilities.windows {
            return vec![];
        }
        match crate::windows::clients() {
            Ok(v) => v,
            Err(e) => {
                let warning = e.to_string();
                if !self.warnings.contains(&warning) {
                    self.warnings.push(warning);
                }
                vec![]
            }
        }
    }
    fn matched(&self, e: &Entry, clients: &[Value]) -> Vec<Value> {
        let classes = self.settings["overrides"][&e.id]
            .get("window_classes")
            .unwrap_or(&e.data["classes"]);
        let tokens: Vec<_> = strings(classes)
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| discovery::folded(s))
            .collect();
        let mut result: Vec<_> = clients
            .iter()
            .filter(|w| {
                w["mapped"] != false
                    && ["app_id", "initial_app_id"].iter().any(|k| {
                        let n = discovery::folded(s(&w[k]));
                        tokens
                            .iter()
                            .any(|t| n == *t || n.starts_with(&format!("{t}._")))
                    })
            })
            .cloned()
            .collect();
        result.sort_by_key(|w| w["focus_order"].as_i64().unwrap_or(9999));
        result
    }
    pub fn query(&mut self, display: &str, offset: usize, limit: Option<usize>) -> Result<Value> {
        let limit = limit.unwrap_or(self.settings["ui"]["max_results"].as_u64().unwrap() as usize);
        if limit == 0 {
            bail!("Invalid result page size");
        }
        if display.is_empty() && !self.last_query.is_empty() {
            self.reset();
        }
        self.last_query = display.into();
        if self.checked.elapsed().as_secs_f64()
            >= self.settings["index"]["refresh_seconds"].as_f64().unwrap()
        {
            self.refresh(false)?;
        }
        let normalized = if display.trim() == "-" {
            "/todo".to_owned()
        } else if display.starts_with('!') && !display.starts_with("!!") {
            format!("/{}", display[1..].trim_start())
        } else {
            display.into()
        };
        let q = normalized.as_str();
        if q.trim() == "/stats" {
            self.current.clear();
            let mut v = self.payload(vec![]);
            v["query"] = json!(display);
            v["statistics"] = if self.demo {
                crate::stats::demo()
            } else {
                self.store.as_ref().unwrap().statistics()?
            };
            return Ok(v);
        }
        let raw_command = q.split_whitespace().next().unwrap_or("");
        if raw_command == "/cpu" || raw_command == "/mem" {
            self.monitor.sort_by(if raw_command == "/mem" {
                "memory"
            } else {
                "cpu"
            });
            let term = q
                .split_once(char::is_whitespace)
                .map(|(_, term)| term.trim())
                .unwrap_or("");
            let normalized = if term.is_empty() {
                "/monitor ".to_owned()
            } else {
                format!("/monitor {term}")
            };
            return self.open_query(&normalized);
        }
        let command = if raw_command == "/find" {
            "/file"
        } else {
            raw_command
        };
        if command == "/timer" {
            let rows = match crate::timer::query(q, self.store.as_ref(), self.demo) {
                Ok(rows) => rows,
                Err(e) => return Ok(self.provider_error("Timer", display, e)),
            };
            let mut value = self.provider_payload(rows, display, offset, limit, false, false);
            value["header"] = json!({"title":"TIMERS","actions":[]});
            return Ok(value);
        }
        if command == "/file" {
            let rows = match self.file_search.files(
                q,
                &self.file_root,
                &self.settings["file_search"],
                self.demo,
            ) {
                Ok(rows) => rows,
                Err(e) => {
                    let mut value = self.provider_error("File", display, e);
                    value["header"] = json!({"title":format!("RESULTS from {}",crate::file_search::display_root(&self.file_root)),"root":self.file_root.to_string_lossy().to_string(),"actions":[{"label":"CHANGE ROOT","mode":"choose-root","id":"/file"}]});
                    return Ok(value);
                }
            };
            let mut value = self.provider_payload(rows, display, offset, limit, false, false);
            value["header"] = json!({"title":format!("RESULTS from {}",crate::file_search::display_root(&self.file_root)),"root":self.file_root.to_string_lossy().to_string(),"actions":[{"label":"CHANGE ROOT","mode":"choose-root","id":"/file"}]});
            return Ok(value);
        }
        if command == "/dir" {
            let rows = match crate::file_search::directories(q, self.demo) {
                Ok(rows) => rows,
                Err(e) => return Ok(self.provider_error("Directory", display, e)),
            };
            let mut value = self.provider_payload(rows, display, offset, limit, false, false);
            value["header"] = json!({"title":"DIRECTORIES · ZOXIDE","actions":[]});
            return Ok(value);
        }
        if command == "/emoji" {
            let rows = match crate::emoji::query(q, self.demo) {
                Ok(rows) => rows,
                Err(e) => return Ok(self.provider_error("Emoji", display, e)),
            };
            let mut value = self.provider_payload(rows, display, offset, limit, false, false);
            value["header"] = json!({"title":"EMOJI","actions":[]});
            return Ok(value);
        }
        if command == "/expand" {
            let rows = match crate::expansions::query(q, self.demo) {
                Ok(rows) => rows,
                Err(error) => return Ok(self.provider_error("Expand", display, error)),
            };
            let mut value = self.provider_payload(rows, display, offset, limit, false, false);
            value["header"] =
                json!({"title":"EXPANSIONS · Enter to edit · Ctrl+Enter to delete","actions":[]});
            return Ok(value);
        }
        if command == "/note" {
            let rows = match self.notes.query(q, self.demo) {
                Ok(rows) => rows,
                Err(e) => return Ok(self.provider_error("Note", display, e)),
            };
            let mut value = self.provider_payload(rows, display, offset, limit, false, false);
            let vault_view = q.trim_start().starts_with("/note vault");
            value["header"] = if vault_view {
                json!({"title":"COPY NOTE TO VAULT","actions":[{"label":"CANCEL","mode":"cancel-note-copy","id":"/note"}]})
            } else {
                json!({"title":"SCRATCH NOTES","actions":[{"label":"NEW","mode":"launch","id":"Note:new"},{"label":"HISTORY","mode":"note-history","id":"/note"},{"label":"VAULT","mode":"note-vault","id":"/note"},{"label":"CHOOSE VAULT","mode":"choose-vault","id":"/note"}]})
            };
            return Ok(value);
        }
        if command == "/monitor" {
            let (rows, closing) = match self.monitor.query(q, self.demo) {
                Ok(rows) => rows,
                Err(e) => return Ok(self.provider_error("Monitor", display, e)),
            };
            let mut value = self.provider_payload(rows, display, offset, limit, false, false);
            value["closing"] = json!(closing);
            let sort = self.monitor.sort().to_uppercase();
            value["header"] = json!({"title":format!("MONITOR · {sort}"),"actions":[{"label":"CPU","mode":"sort-cpu","id":"/monitor","active":self.monitor.sort()=="cpu"},{"label":"MEMORY","mode":"sort-memory","id":"/monitor","active":self.monitor.sort()=="memory"}]});
            return Ok(value);
        }
        if command == "/bw" {
            let rows = match crate::bitwarden::query(q, self.demo) {
                Ok(rows) => rows,
                Err(e) => return Ok(self.provider_error("Bitwarden", display, e)),
            };
            let mut value = self.provider_payload(rows, display, offset, limit, false, false);
            value["header"] = json!({"title":"BITWARDEN","actions":[]});
            return Ok(value);
        }
        let run = q == "/run" || q.starts_with("/run ") || q.starts_with("!!");
        let running = q.starts_with('?')
            || ["/gui"]
                .iter()
                .any(|prefix| q == *prefix || q.starts_with(&format!("{prefix} ")));
        let search = if let Some(s) = q.strip_prefix("!!") {
            s.trim_start()
        } else if run {
            q[4..].trim_start()
        } else if let Some(s) = q.strip_prefix('?') {
            s.trim_start()
        } else if running {
            q.split_once(' ').map(|(_, v)| v.trim_start()).unwrap_or("")
        } else {
            q
        };
        if command != "/tabs" {
            self.browser.scope = None;
        }
        let enabled = strings(&self.settings["plugins"]["enabled"]);
        if command == "/voice" {
            let rows = match crate::voice::query(q, self.demo) {
                Ok(rows) => rows,
                Err(e) => return Ok(self.provider_error("Voice", display, e)),
            };
            return Ok(self.provider_payload(rows, display, offset, limit, false, false));
        }
        let capture_provider =
            if ["/grab", "/shot", "/screenshot", "/ss", "/snap"].contains(&command) {
                "Screenshots"
            } else if ["/rec", "/sr", "/screenrecord"].contains(&command) {
                "Recorder"
            } else {
                ""
            };
        if !capture_provider.is_empty() && enabled.iter().any(|s| s == capture_provider) {
            let rows = match self
                .captures
                .query(capture_provider, q, &self.settings, self.demo)
            {
                Ok(rows) => rows,
                Err(e) => return Ok(self.provider_error(capture_provider, display, e)),
            };
            let mut value =
                self.provider_payload(rows, display, offset, limit, self.captures.pending, false);
            if capture_provider == "Recorder" && !q.split_whitespace().any(|part| part == "history")
            {
                let options = &self.settings["plugin_settings"]["Recorder"];
                value["header"] = json!({"title":"RECORDING", "actions":[
                    {"label":"Record voice", "id":q, "mode":"record-microphone", "toggle":true, "active":options["microphone"] == true},
                    {"label":"Record system", "id":q, "mode":"record-audio", "toggle":true, "active":options["audio"] == true}
                ]});
            }
            return Ok(value);
        }
        if enabled.iter().any(|s| s == "Clipboard") && command == "/cliphist" {
            let rows = match self.clipboard.query(q, &self.settings, self.demo) {
                Ok(rows) => rows,
                Err(e) => return Ok(self.provider_error("Clipboard", display, e)),
            };
            return Ok(self.provider_payload(
                rows,
                display,
                offset,
                limit,
                self.clipboard.pending,
                false,
            ));
        }
        if enabled.iter().any(|s| s == "Todo") && ["/todo", "/reading"].contains(&command) {
            let mut state = match self.todo.state(json!({}), &self.settings, self.demo) {
                Ok(state) => state,
                Err(e) => return Ok(self.provider_error("Todo", display, e)),
            };
            state["collection"] = json!(if command == "/reading" {
                "reading"
            } else {
                "todo"
            });
            let mut v = self.payload(vec![]);
            v["query"] = json!(display);
            v["app_mode"] = json!("Todo");
            v["app_state"] = state;
            return Ok(v);
        }
        if enabled.iter().any(|s| s == "Words") && ["/define", "/synonym"].contains(&command) {
            let rows = self.words.query(q, &self.settings, self.demo);
            let mut counts = serde_json::Map::new();
            for row in &rows {
                if row.data["disabled"] == true {
                    continue;
                }
                if let Some(source) = row.data["word_source"].as_str() {
                    let count = counts.get(source).and_then(Value::as_u64).unwrap_or(0);
                    counts.insert(source.to_owned(), json!(count + 1));
                }
            }
            let mut value =
                self.provider_payload(rows, display, offset, limit, self.words.pending, false);
            value["word_source_counts"] = json!(counts);
            if self.words.cache_hits > 0 {
                value["word_cache_hits"] = json!(self.words.cache_hits);
            }
            return Ok(value);
        }
        if enabled.iter().any(|s| s == "Browser")
            && ["/browser", "/zen", "/tabs", "/history"].contains(&command)
        {
            return match self.browser.query(q, &self.settings, self.demo) {
                Ok(rows) => Ok(self.provider_payload(rows, display, offset, limit, false, true)),
                Err(e) => {
                    let mut v = self.payload(vec![]);
                    v["query"] = json!(display);
                    v["message"] = json!(format!("Browser: {e}"));
                    v["app_mode"] = json!("");
                    Ok(v)
                }
            };
        }
        if q.starts_with('/') && !run && !running {
            let mut rows = crate::menus::query(q, &self.settings)?.unwrap_or_default();
            for row in &mut rows {
                row["icon"] = json!(self.icons.resolve(s(&row["icon"])));
            }
            let _ = self.icons.flush();
            let mut v = self.payload(rows);
            v["query"] = json!(display);
            return Ok(v);
        }
        // Rank native discovery, commands and optional Browser tabs.
        let mut candidates: Vec<Entry> = self
            .entries
            .iter()
            .filter(|e| e.provider == if run { "commands" } else { "apps" })
            .cloned()
            .collect();
        if !search.trim().is_empty() && !run && !running {
            candidates.extend(crate::menus::entries(&self.settings));
        }
        if enabled.iter().any(|s| s == "Browser") && !run && !running {
            match self.browser.query(q, &self.settings, self.demo) {
                Ok(rows) => candidates.extend(rows),
                Err(e) => {
                    let warning = format!("Browser: {e}");
                    if !self.warnings.contains(&warning) {
                        self.warnings.push(warning);
                    }
                }
            }
        }
        let clients = self.clients();
        let live: std::collections::HashSet<_> =
            clients.iter().map(|c| s(&c["id"]).to_owned()).collect();
        self.closing.retain(|_, addresses| {
            addresses.retain(|a| live.contains(a));
            !addresses.is_empty()
        });
        let counts = self
            .store
            .as_ref()
            .map(|s| s.counts())
            .transpose()?
            .unwrap_or(json!({}));
        if self.ranking.is_none() {
            self.ranking = Some(
                self.store
                    .as_ref()
                    .map(|s| s.scores())
                    .transpose()?
                    .unwrap_or_default(),
            );
        }
        let scores = self.ranking.as_ref().unwrap();
        let aliases = |e: &Entry| {
            e.data
                .get("aliases")
                .map(strings)
                .unwrap_or_else(|| vec![e.id.clone()])
        };
        let mut ranked: Vec<_> = candidates
            .into_iter()
            .filter(|e| !running || !self.matched(e, &clients).is_empty())
            .filter_map(|e| {
                discovery::score(search, &e).map(|score| {
                    let keys = aliases(&e);
                    let count = keys
                        .iter()
                        .map(|k| counts[format!("launch:{k}")]["count"].as_i64().unwrap_or(0))
                        .sum::<i64>();
                    let history = keys
                        .iter()
                        .map(|k| scores.get(k).copied().unwrap_or(0.))
                        .sum::<f64>();
                    (e, score, count, history)
                })
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| b.3.total_cmp(&a.3))
                .then_with(|| discovery::folded(&a.0.name).cmp(&discovery::folded(&b.0.name)))
                .then_with(|| a.0.id.cmp(&b.0.id))
        });
        if let Some(order) = self.orders.get(q) {
            let positions: HashMap<_, _> =
                order.iter().enumerate().map(|(i, id)| (id, i)).collect();
            ranked.sort_by_key(|r| positions.get(&r.0.id).copied().unwrap_or(order.len()));
        }
        self.orders
            .insert(q.into(), ranked.iter().map(|r| r.0.id.clone()).collect());
        let ranked: Vec<_> = ranked
            .into_iter()
            .map(|(e, _, count, _)| (e, count))
            .collect();
        if offset == 0 || self.page_query != q {
            self.page_query = q.into();
            self.page = ranked;
        }
        let page: Vec<_> = self.page.iter().skip(offset).take(limit).cloned().collect();
        let total = self.page.len();
        if offset == 0 {
            self.current.clear();
        }
        let mut rows = vec![];
        for (e, count) in page {
            let windows = self.matched(&e, &clients).len();
            let row = json!({"id":e.id,"name":e.name,"subtitle":e.subtitle,"icon":self.icons.resolve(&e.icon),"provider":e.provider,"favicon":s(&e.data["favicon"]),"keep_open":e.data["keep_open"]==true,"count":count,"windows":windows});
            self.current.insert(e.id.clone(), e);
            rows.push(row);
        }
        let _ = self.icons.flush();
        let next = offset + rows.len();
        let mut v = self.payload(rows);
        for(k,value)in json!({"total":total,"query":display,"offset":offset,"next_offset":next,"has_more":next<total,"closing":self.closing.keys().collect::<Vec<_>>()}).as_object().unwrap(){v[k]=value.clone();}
        Ok(v)
    }
    // Change the query and its contents in one response: never leave the command
    // tile visible while the frontend waits for a second, debounced search.
    fn open_query(&mut self, query: &str) -> Result<Value> {
        let mut value = self.query(query, 0, None)?;
        value["edit_query"] = json!(query);
        Ok(value)
    }
    fn activate(&mut self, req: Value) -> Result<Value> {
        let id = s(&req["id"]);
        let mode = req["mode"].as_str().unwrap_or("launch");
        if ["record-microphone", "record-audio"].contains(&mode) {
            let key = mode.strip_prefix("record-").unwrap();
            let enabled = self.settings["plugin_settings"]["Recorder"][key] != true;
            if self.demo {
                self.settings["plugin_settings"]["Recorder"][key] = json!(enabled);
            } else {
                self.settings = crate::settings::save(
                    &format!("plugin_settings.Recorder.{key}"),
                    &enabled.to_string(),
                )?;
            }
            return self.open_query(id);
        }
        if id.starts_with("Expand:") {
            let entry = self
                .current
                .get(id)
                .context("Expansion is no longer selected")?
                .clone();
            anyhow::ensure!(
                mode != "save-expansion" || req["original"] == entry.data["original"],
                "Expansion changed; cancel and reopen the editor"
            );
            let original = if mode == "save-expansion" {
                req["original"].clone()
            } else {
                entry.data["original"].clone()
            };
            if mode == "close" || mode == "save-expansion" {
                if id == "Expand:new" && mode == "close" {
                    return Ok(json!({"kind":"message","message":"Select an expansion to delete"}));
                }
                if self.demo {
                    return Ok(
                        json!({"kind":"message","message":"Demo mode does not change expansions"}),
                    );
                }
                crate::expansions::request(
                    json!({"action":if mode == "close" {"delete"} else {"save"},"original":original,"trigger":req["trigger"],"expansion":req["expansion"]}),
                )?;
                let query = s(&req["query"]);
                return self.open_query(if query.starts_with("/expand") {
                    query
                } else {
                    "/expand"
                });
            }
            return Ok(json!({"kind":"expansion-editor","id":id,"original":original}));
        }
        if mode == "set-root" {
            let raw = s(&req["path"]);
            let path = if raw.starts_with("file:") {
                gio::File::for_uri(raw)
                    .path()
                    .context("Folder chooser returned an invalid path")?
            } else {
                std::path::PathBuf::from(raw)
            };
            anyhow::ensure!(
                path.is_absolute() && path.is_dir(),
                "Choose an existing folder as the search root"
            );
            self.file_root = path.canonicalize()?;
            if !self.demo {
                self.settings = crate::settings::save(
                    "file_search.root",
                    &serde_json::to_string(&self.file_root.to_string_lossy().to_string())?,
                )?;
            }
            let mut value = self.open_query("/file ")?;
            value["message"] = json!(format!(
                "Search root changed to {}",
                crate::file_search::display_root(&self.file_root)
            ));
            return Ok(value);
        }
        if mode == "set-vault" {
            let raw = s(&req["path"]);
            let path = if raw.starts_with("file:") {
                gio::File::for_uri(raw)
                    .path()
                    .context("Folder chooser returned an invalid path")?
            } else {
                std::path::PathBuf::from(raw)
            };
            self.notes.set_vault(&path)?;
            return self.open_query("/note");
        }
        match mode {
            "sort-cpu" => {
                self.monitor.sort_by("cpu");
                return self.open_query("/monitor ");
            }
            "sort-memory" => {
                self.monitor.sort_by("memory");
                return self.open_query("/monitor ");
            }
            "note-history" => return self.open_query("/note history"),
            "note-vault" => return self.open_query("/note vault"),
            "cancel-note-copy" => {
                self.notes.cancel_promotion();
                return self.open_query("/note");
            }
            _ => {}
        }
        if mode == "copy" {
            let entry = self
                .current
                .get(id)
                .filter(|entry| {
                    ["Screenshots", "Recorder"].contains(&entry.provider.as_str())
                        && (entry.data["file"].is_string()
                            || entry.data["clip_id"].is_string()
                            || (self.demo
                                && entry
                                    .id
                                    .starts_with(&format!("{}:demo-file-", entry.provider))))
                })
                .context("Select an image or video to copy")?;
            return self
                .captures
                .activate(entry, &self.settings, mode, self.demo);
        }
        if !["launch", "switch", "close", "username"].contains(&mode) {
            bail!("Unknown action");
        }
        if id == "/refresh" && mode != "close" {
            self.refresh(true)?;
            if self.tracking_enabled() {
                if let Some(s) = &self.store {
                    s.record("index", "refresh", true)?;
                }
            }
            let mut v = self.query("", 0, None)?;
            v["reset_query"] = json!(true);
            v["message"] = json!(format!("Index rebuilt · {} entries", self.entries.len()));
            return Ok(v);
        }
        if id.starts_with('/') {
            if mode == "close" {
                return Ok(
                    json!({"kind":"message","message":"Ctrl+Enter closes applications, not launcher commands"}),
                );
            }
            if ["/config", "/bindings", "/shortcuts"].contains(&id) {
                return self.open_query(id);
            }
            if id == "/config/error" || id.starts_with("/shortcuts/") {
                return Ok(
                    json!({"kind":"message","message":"Type a command or press Escape to close"}),
                );
            }
            if let Some(text) = id.strip_prefix("/config ") {
                let (key, value) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
                if value.is_empty()
                    && crate::settings::featured(&self.settings)
                        .iter()
                        .any(|(k, _)| k == key)
                {
                    return Ok(
                        json!({"kind":"edit","text":format!("{id} {}",crate::settings::display(crate::settings::get(&self.settings,key))),"select_value":true}),
                    );
                }
                if self.demo {
                    return Ok(
                        json!({"kind":"message","message":"Demo mode does not change your config"}),
                    );
                }
                crate::settings::save(key, value)?;
                *self = Self::new(self.demo)?;
                let mut v = self.query("/config", 0, None)?;
                v["edit_query"] = json!("/config");
                v["apply_config"] = json!(true);
                v["message"] = json!(format!("Saved {key} in runtjik.conf"));
                return Ok(v);
            }
            if ["/expand", "/voice", "/stats", "/run", "/gui", "/timer"].contains(&id) {
                let query = if id == "/stats" {
                    id.to_owned()
                } else {
                    format!("{id} ")
                };
                return self.open_query(&query);
            }
            if let Some(c) = crate::menus::commands(&self.settings).iter().find(|c| {
                !c.provider.is_empty() && (c.id == id || c.aliases.iter().any(|a| a == id))
            }) {
                let query = if c.app {
                    id.to_owned()
                } else {
                    format!("{id} ")
                };
                return self.open_query(&query);
            }
        }
        let entry = self
            .current
            .get(id)
            .or_else(|| self.entries.iter().find(|e| e.id == id))
            .cloned();
        if let Some(entry) = entry.as_ref() {
            if ["File", "Directory"].contains(&entry.provider.as_str()) {
                if self.demo {
                    return Ok(
                        json!({"kind":"message","message":"Demo mode leaves files unchanged"}),
                    );
                }
                let path = std::path::Path::new(s(&entry.data["path"]));
                if mode == "close" {
                    let trashed = crate::trash::action(Some(path))?;
                    let mut value = self.open_query(&self.last_query.clone())?;
                    value["message"] = trashed["message"].clone();
                    return Ok(value);
                }
                crate::file_search::open(path)?;
                return Ok(json!({"kind":"activated","message":"Opened with the default handler"}));
            }
            if entry.provider == "Note" {
                let action = self.notes.activate(entry, mode, self.demo)?;
                if let Some(query) = action["open_query"].as_str() {
                    let mut value = self.open_query(query)?;
                    if !action["message"].is_null() {
                        value["message"] = action["message"].clone();
                    }
                    return Ok(value);
                }
                return Ok(action);
            }
            if entry.provider == "Monitor" && mode == "close" {
                let message = self.monitor.close(entry, self.demo)?;
                let mut value = self.open_query(&self.last_query.clone())?;
                value["message"] = json!(message);
                return Ok(value);
            }
            if entry.provider == "Emoji" {
                if mode == "switch" {
                    if entry.data["has_tones"] == true {
                        return self
                            .open_query(&format!("/emoji tones {}", s(&entry.data["base_name"])));
                    }
                    return Ok(json!({
                        "kind":"message",
                        "message":"No skin tone variants for this emoji"
                    }));
                }
                if self.demo {
                    return Ok(
                        json!({"kind":"message","message":"Demo mode leaves the clipboard unchanged"}),
                    );
                }
                crate::clipboard::copy_text(s(&entry.data["emoji"]), false)?;
                return Ok(
                    json!({"kind":"activated","message":"Emoji copied","close_launcher":true}),
                );
            }
            if entry.provider == "Bitwarden" {
                return crate::bitwarden::activate(entry, mode, self.demo);
            }
        }
        if let Some(e) = &entry {
            if e.provider == "Timer" {
                let timer_id = s(&e.data["timer_id"]);
                if !timer_id.is_empty() {
                    if mode == "close" {
                        if self.demo {
                            return Ok(
                                json!({"kind":"message","message":"Demo timers are synthetic"}),
                            );
                        }
                        let removed =
                            crate::timer::cancel(self.store.as_ref().unwrap(), timer_id, false)?;
                        let mut value = self.open_query("/timer")?;
                        value["message"] = json!(if removed {
                            "Timer cancelled"
                        } else {
                            "That timer has already finished"
                        });
                        if removed {
                            value["removed_entry"] = json!(e.id);
                        }
                        return Ok(value);
                    }
                    if e.data["finished"] == true {
                        if let Err(error) =
                            crate::timer::retry(self.store.as_ref().unwrap(), timer_id, self.demo)
                        {
                            return Ok(json!({"kind":"message","message":error.to_string()}));
                        }
                        let mut value = self.open_query("/timer")?;
                        value["message"] = json!("Timer alert sent");
                        value["removed_entry"] = json!(e.id);
                        return Ok(value);
                    }
                    return Ok(
                        json!({"kind":"message","message":"Active timer · Ctrl+Enter cancels"}),
                    );
                }
                if let Some((seconds, label)) = crate::timer::parse_set_row(e) {
                    if self.demo {
                        let mut value = self.open_query("/timer")?;
                        value["message"] = json!(format!(
                            "Demo: {} timer set",
                            crate::timer::duration_label(seconds)
                        ));
                        return Ok(value);
                    }
                    match crate::timer::start(self.store.as_ref().unwrap(), seconds, &label) {
                        Ok(_) => {
                            let mut value = self.open_query("/timer")?;
                            value["message"] = json!(format!("Timer set · {label}"));
                            return Ok(value);
                        }
                        Err(error) => {
                            return Ok(json!({"kind":"message","message":error.to_string()}))
                        }
                    }
                }
            }
            if e.provider == "Voice" {
                let action = crate::voice::activate(e, mode, &self.settings, self.demo)?;
                if let Some(query) = action["open_query"].as_str() {
                    return self.open_query(query);
                }
                if mode == "close" {
                    let query = self.last_query.clone();
                    let mut value = self.query(&query, 0, None)?;
                    value["message"] = action["message"].clone();
                    value["removed_entry"] = action["removed_entry"].clone();
                    return Ok(value);
                }
                return Ok(action);
            }
            if ["Screenshots", "Recorder"].contains(&e.provider.as_str()) {
                if mode == "close"
                    && e.data.get("file").is_none()
                    && e.data.get("clip_id").is_none()
                {
                    return Ok(
                        json!({"kind":"message", "message":"Ctrl+Enter is available for applications and commands with matching windows"}),
                    );
                }
                let action = self.captures.activate(e, &self.settings, mode, self.demo)?;
                if mode == "close" {
                    let q = if self.last_query.is_empty() {
                        s(&action["refresh_query"]).to_owned()
                    } else {
                        self.last_query.clone()
                    };
                    let mut v = self.query(&q, 0, None)?;
                    v["message"] = action["message"].clone();
                    if !self.demo {
                        v["removed_entry"] = json!(e.id);
                    }
                    return Ok(v);
                }
                return Ok(action);
            }
            if e.provider == "Clipboard" {
                let action = crate::clipboard::Clipboard::activate(e, mode, self.demo)?;
                if mode == "close" {
                    let q = if self.last_query.is_empty() {
                        "/cliphist".into()
                    } else {
                        self.last_query.clone()
                    };
                    let mut value = self.query(&q, 0, None)?;
                    value["message"] = action["message"].clone();
                    if !self.demo && e.data["clip_id"].is_string() {
                        value["removed_entry"] = json!(e.id);
                    }
                    return Ok(value);
                }
                return Ok(action);
            }
            if e.provider == "Words" {
                if mode == "close" {
                    return Ok(
                        json!({"kind":"message","message":"Ctrl+Enter is available for applications and commands with matching windows"}),
                    );
                }
                if let Some(query) = e.data["lookup_query"].as_str().map(str::to_owned) {
                    return self.open_query(&query);
                }
                return crate::words::Words::activate(e, self.demo);
            }
            if e.provider == "Browser" {
                if mode == "switch" && !e.data["tab"].is_null() && e.data["history"] != true {
                    anyhow::ensure!(
                        strings(&self.settings["plugins"]["enabled"])
                            .iter()
                            .any(|p| p == "Todo"),
                        "Enable Todo to save reading items"
                    );
                    let draft = self.reading.begin(e, &self.settings, self.demo)?;
                    let mut value = self.open_query("/reading")?;
                    value["app_state"]
                        .as_object_mut()
                        .ok_or_else(|| anyhow::anyhow!("Reading is unavailable"))?
                        .extend(draft.as_object().unwrap().clone());
                    return Ok(value);
                }
                let action = self.browser.activate(e, mode, self.demo)?;
                if mode == "close" {
                    let q = if self.last_query.is_empty() {
                        "/tabs".into()
                    } else {
                        self.last_query.clone()
                    };
                    let mut value = self.query(&q, 0, None)?;
                    value["message"] = action["message"].clone();
                    return Ok(value);
                }
                return Ok(action);
            }
        }
        if mode != "close" {
            match id {
                "/pin" => return Ok(json!({"kind":"pin"})),
                "/grid" | "/list" => return Ok(json!({"kind":"view","view":&id[1..]})),
                "/run" | "/gui" | "/timer" | "/file" | "/find" | "/dir" | "/emoji" | "/note"
                | "/monitor" | "/cpu" | "/mem" | "/bw" | "?" => {
                    return self.open_query(&format!("{id} "))
                }
                _ => {}
            }
        }
        if let Some(e) = entry
            .as_ref()
            .filter(|e| ["apps", "commands"].contains(&e.provider.as_str()) && mode == "switch")
        {
            let prepare = req["prepare"] == true;
            let observe = prepare && req["observe"] == true;
            let clients = if self.demo {
                config::contract()["demo_windows"]
                    .as_array()
                    .unwrap()
                    .clone()
            } else {
                match crate::windows::clients() {
                    Ok(clients) => clients,
                    Err(_) if observe => vec![],
                    Err(e) => return Err(e),
                }
            };
            let clients = self.matched(e, &clients);
            if clients.is_empty() && !observe {
                if prepare {
                    return Ok(json!({"kind":"launch","entry":id}));
                }
                let mut launch = req.clone();
                launch["mode"] = json!("launch");
                return self.activate(launch);
            }
            let address = s(&req["address"]);
            if address.is_empty() && (prepare || clients.len() > 1) {
                let icon = self.icons.resolve(&e.icon);
                let rows: Vec<_> = clients.iter().map(|w| json!({"id":w["id"],"name":if s(&w["title"]).is_empty(){e.name.as_str()}else{s(&w["title"])},"subtitle":format!("Workspace {}",s(&w["workspace"]["name"])),"icon":icon,"provider":"windows","windows":0,"count":0})).collect();
                return Ok(
                    json!({"kind":"windows","entry":id,"auto_select":!observe && prepare && clients.len()==1,"results":rows}),
                );
            }
            let target = if address.is_empty() {
                &clients[0]
            } else {
                clients.iter().find(|w| w["id"] == address).ok_or_else(|| {
                    anyhow::anyhow!("That window has closed; select another instance")
                })?
            };
            if !self.demo {
                crate::windows::focus(target)?;
            }
            if self.tracking_enabled() {
                if let Some(store) = &self.store {
                    store.record(id, "switch", true)?;
                }
            }
            return Ok(json!({"kind":"activated","demo":self.demo}));
        }
        let pending = if let (Some(store), Some(e)) = (&self.store, &entry) {
            crate::close::pending(store, e, &self.settings)?
        } else {
            crate::close::Pending::new()
        };
        if let Some(e) = entry.as_ref().filter(|e| {
            ["apps", "commands"].contains(&e.provider.as_str())
                && (mode == "close" || (mode == "launch" && !pending.is_empty()))
        }) {
            if self.demo {
                return Ok(
                    json!({"kind":"activated","action":"close","demo":true,"message":format!("Demo: graceful close request for {}",e.name)}),
                );
            }
            let clients = if mode == "launch" {
                vec![]
            } else {
                self.matched(e, &crate::windows::clients()?)
            };
            return crate::close::close(
                e,
                self.store.as_ref().unwrap(),
                &self.settings,
                clients,
                s(&req["address"]),
                pending,
                &mut self.closing,
            );
        }
        if let Some(e) = entry.as_ref().filter(|e| {
            ["apps", "commands"].contains(&e.provider.as_str())
                && mode == "launch"
                && pending.is_empty()
        }) {
            if !self.demo {
                let override_args = self.settings["overrides"][id].get("argv").map(strings);
                let args = override_args.unwrap_or_else(|| {
                    if e.provider == "apps" {
                        vec!["gio".into(), "launch".into(), s(&e.data["path"]).into()]
                    } else {
                        let mut args = if e.data["gui"] == true {
                            vec![]
                        } else {
                            strings(&self.settings["launch"]["terminal"])
                        };
                        args.extend(strings(&e.data["argv"]));
                        args
                    }
                });
                let result = dispatch(args, &self.settings);
                if let Some(store) = &self.store {
                    if self.tracking_enabled() {
                        store.record(id, mode, result.is_ok())?;
                    }
                }
                result?;
            }
            return Ok(json!({"kind":"activated","demo":self.demo}));
        }
        if ["/shortcuts", "/bindings", "/config"].contains(&id) {
            return self.open_query(id);
        }
        if id.starts_with("/shortcuts/") || id == "/config/error" {
            return Ok(
                json!({"kind":"message","message":"Type a command or press Escape to close"}),
            );
        }
        bail!("Entry is no longer available; refresh the index")
    }

    pub fn handle(&mut self, req: Value) -> Result<Option<Value>> {
        let result = match s(&req["op"]) {
            "query" => {
                let offset = match req.get("offset") {
                    None => 0,
                    Some(v) => v
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("Invalid result offset"))?
                        as usize,
                };
                let limit = match req.get("limit") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(
                        v.as_u64()
                            .ok_or_else(|| anyhow::anyhow!("Invalid result page size"))?
                            as usize,
                    ),
                };
                self.query(s(&req["query"]), offset, limit)?
            }
            "remember-command" => {
                let query = crate::util::s(&req["query"]);
                if self.settings["history"]["enabled"] == true
                    && !query.trim_start().starts_with("/bw ")
                    && query.trim() != "/bw"
                {
                    if let Some(s) = &self.store {
                        s.remember(crate::util::s(&req["query"]))?;
                    }
                }
                return Ok(None);
            }
            "resume" => {
                let settings = config::load()?;
                if settings != self.settings {
                    *self = Self::new(self.demo)?;
                }
                self.reset();
                let mut v = self.initial()?;
                v["apply_config"] = json!(true);
                v
            }
            "activate" => self.activate(req)?,
            "app-action" if req["provider"] == "Todo" => {
                anyhow::ensure!(
                    strings(&self.settings["plugins"]["enabled"])
                        .iter()
                        .any(|p| p == "Todo"),
                    "Todo is disabled"
                );
                let state = if ["reading-save", "reading-cancel"].contains(&s(&req["action"])) {
                    self.reading
                        .action(req, &mut self.todo, &self.settings, self.demo)?
                } else {
                    self.todo.state(req, &self.settings, self.demo)?
                };
                json!({"kind":"app-state", "app_state":state})
            }
            "undo-trash" => {
                let message = if self.demo {
                    json!("Demo mode leaves Trash unchanged")
                } else {
                    crate::trash::action(None)?["message"].clone()
                };
                let mut v = self.query(s(&req["query"]), 0, None)?;
                v["message"] = message;
                v
            }
            "app-action" => bail!("App provider is unavailable"),
            _ => bail!("Unknown operation"),
        };
        Ok(Some(result))
    }
}
