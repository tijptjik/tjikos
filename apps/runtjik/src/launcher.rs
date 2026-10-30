use crate::{
    compat::Host,
    config,
    discovery::{self, Entry},
    icons::Icons,
    store::Store,
    util::*,
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::{collections::HashMap, time::Instant};
pub struct Launcher {
    pub entries: Vec<Entry>,
    settings: Value,
    demo: bool,
    store: Option<Store>,
    icons: Icons,
    host: Option<Host>,
    warnings: Vec<String>,
    checked: Instant,
    current: HashMap<String, Entry>,
    ranking: Option<HashMap<String, f64>>,
    orders: HashMap<String, Vec<String>>,
    last_query: String,
    page_query: String,
    page: Vec<(Entry, i64)>,
    closing: crate::close::Tracking,
    custom: bool,
    reset_scope: bool,
    words: crate::words::Words,
    browser: crate::browser::Browser,
}
impl Launcher {
    pub fn new(demo: bool) -> Result<Self> {
        let settings = config::load()?;
        let icons = Icons::new(&settings);
        let store = if demo { None } else { Some(Store::open()?) };
        let custom = strings(&settings["plugins"]["enabled"]).iter().any(|n| {
            ![
                "Words",
                "Clipboard",
                "Recorder",
                "Screenshots",
                "Todo",
                "Browser",
            ]
            .contains(&n.as_str())
        });
        let mut app = Self {
            entries: vec![],
            settings,
            demo,
            store,
            icons,
            host: None,
            warnings: vec![],
            checked: Instant::now(),
            current: HashMap::new(),
            ranking: None,
            orders: HashMap::new(),
            last_query: String::new(),
            page_query: String::new(),
            page: vec![],
            closing: crate::close::Tracking::new(),
            custom,
            reset_scope: true,
            words: crate::words::Words::default(),
            browser: crate::browser::Browser::default(),
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
        Ok(json!(if self.settings["history"]["enabled"] == true {
            self.store
                .as_ref()
                .map(|s| s.history())
                .transpose()?
                .unwrap_or_default()
        } else {
            vec![]
        }))
    }
    pub fn initial(&mut self) -> Result<Value> {
        let mut v = self.query("", 0, None)?;
        v["command_history"] = self.history()?;
        Ok(v)
    }
    fn bridge(&mut self, mut req: Value) -> Result<Value> {
        req["reset_scope"] = json!(self.reset_scope);
        self.reset_scope = false;
        req["native_providers"] = json!(["Words", "Browser"]);
        req["settings"] = self.settings.clone();
        req["entries"] = serde_json::to_value(&self.entries)?;
        req["demo"] = json!(self.demo);
        if self.host.is_none() {
            self.host = Some(Host::start()?);
        }
        match self.host.as_mut().unwrap().request(&req) {
            Ok(v) => Ok(v),
            Err(e) => {
                self.host = None;
                Err(e)
            }
        }
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
        let rows:Vec<_>=entries.into_iter().skip(offset).take(limit).map(|e|json!({"id":e.id,"name":e.name,"subtitle":e.subtitle,"provider":e.provider,"icon":self.icons.resolve(&e.icon),"favicon":s(&e.data["favicon"]),"hide_launcher":e.data["capture"].as_str().is_some_and(|s|!s.is_empty()),"keep_open":e.data["keep_open"]==true,"thumbnail":s(&e.data["thumbnail"]),"empty_state":e.data["empty_state"]==true,"disabled":e.data["disabled"]==true,"count":0,"windows":0})).collect();
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
        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
            return vec![];
        }
        match command(&["hyprctl", "-j", "clients"])
            .and_then(|s| Ok(serde_json::from_str::<Vec<Value>>(&s)?))
        {
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
                    && ["class", "initialClass"].iter().any(|k| {
                        let n = discovery::folded(s(&w[k]));
                        tokens
                            .iter()
                            .any(|t| n == *t || n.starts_with(&format!("{t}._")))
                    })
            })
            .cloned()
            .collect();
        result.sort_by_key(|w| w["focusHistoryID"].as_i64().unwrap_or(9999));
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
        let normalized = if display.starts_with('!') && !display.starts_with("!!") {
            format!("/{}", display[1..].trim_start())
        } else {
            display.into()
        };
        let q = normalized.as_str();
        let run = q == "/run" || q.starts_with("/run ") || q.starts_with("!!");
        let running = q.starts_with('?') || q == "/processes" || q.starts_with("/processes ");
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
        let command = q.split_whitespace().next().unwrap_or("");
        if command != "/tabs" {
            self.browser.scope = None;
        }
        let enabled = strings(&self.settings["plugins"]["enabled"]);
        if enabled.iter().any(|s| s == "Words") && ["/define", "/synonym"].contains(&command) {
            let rows = self.words.query(q, &self.settings, self.demo);
            return Ok(self.provider_payload(
                rows,
                display,
                offset,
                limit,
                self.words.pending,
                false,
            ));
        }
        if enabled.iter().any(|s| s == "Browser")
            && ["/browser", "/zen", "/tabs"].contains(&command)
        {
            return match self.browser.query(q, self.demo) {
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
        if q.starts_with('/') && !run && !running && !self.custom {
            if let Some(mut rows) = crate::menus::query(q, &self.settings)? {
                for row in &mut rows {
                    row["icon"] = json!(self.icons.resolve(s(&row["icon"])));
                }
                let _ = self.icons.flush();
                let mut v = self.payload(rows);
                v["query"] = json!(display);
                return Ok(v);
            }
        }
        if q.starts_with('/') && !run && !running {
            return self
                .bridge(json!({"op":"query","query":display,"offset":offset,"limit":limit}));
        }
        self.reset_scope = true;
        // Rank discovery, commands and optional Browser tabs; extensions use the reference path.
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
            match self.browser.query(q, self.demo) {
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
        let live: std::collections::HashSet<_> = clients
            .iter()
            .map(|c| s(&c["address"]).to_owned())
            .collect();
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
    fn activate(&mut self, req: Value) -> Result<Value> {
        let id = s(&req["id"]);
        let mode = req["mode"].as_str().unwrap_or("launch");
        if !["launch", "switch", "close"].contains(&mode) {
            bail!("Unknown action");
        }
        if id == "/refresh" && mode != "close" {
            self.refresh(true)?;
            if self.settings["history"]["enabled"] == true {
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
                return Ok(json!({"kind":"edit","text":id}));
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
            if id == "/stats" {
                let counts = self
                    .store
                    .as_ref()
                    .map(|s| s.counts())
                    .transpose()?
                    .unwrap_or(json!({}));
                let total = |prefix: &str| {
                    counts
                        .as_object()
                        .unwrap()
                        .iter()
                        .filter(|(k, _)| k.starts_with(prefix))
                        .map(|(_, v)| v["count"].as_i64().unwrap_or(0))
                        .sum::<i64>()
                };
                return Ok(
                    json!({"kind":"message","message":format!("{} launches · {} window switches · {} indexed entries",total("launch:"),total("switch:"),self.entries.len())}),
                );
            }
            if let Some(c) = crate::menus::commands(&self.settings).iter().find(|c| {
                !c.provider.is_empty() && (c.id == id || c.aliases.iter().any(|a| a == id))
            }) {
                if !c.app {
                    return Ok(json!({"kind":"edit","text":format!("{id} ")}));
                }
            }
        }
        let entry = self
            .current
            .get(id)
            .or_else(|| self.entries.iter().find(|e| e.id == id))
            .cloned();
        if let Some(e) = &entry {
            if e.provider == "Words" {
                if mode == "close" {
                    return Ok(
                        json!({"kind":"message","message":"Ctrl+Enter is available for applications and commands with matching windows"}),
                    );
                }
                return crate::words::Words::activate(e, self.demo);
            }
            if e.provider == "Browser" {
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
                "/run" | "/processes" | "?" => {
                    return Ok(json!({"kind":"edit","text":format!("{id} ")}))
                }
                _ => {}
            }
        }
        if let Some(e) = entry
            .as_ref()
            .filter(|e| ["apps", "commands"].contains(&e.provider.as_str()) && mode == "switch")
        {
            let clients = if self.demo {
                config::contract()["demo_windows"]
                    .as_array()
                    .unwrap()
                    .clone()
            } else {
                crate::windows::clients()?
            };
            let clients = self.matched(e, &clients);
            let prepare = req["prepare"] == true;
            if clients.is_empty() {
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
                let rows: Vec<_> = clients.iter().map(|w| json!({"id":w["address"],"name":if s(&w["title"]).is_empty(){e.name.as_str()}else{s(&w["title"])},"subtitle":format!("Workspace {}",s(&w["workspace"]["name"])),"icon":icon,"provider":"windows","windows":0,"count":0})).collect();
                return Ok(
                    json!({"kind":"windows","entry":id,"auto_select":prepare && clients.len()==1,"results":rows}),
                );
            }
            let target = if address.is_empty() {
                &clients[0]
            } else {
                clients
                    .iter()
                    .find(|w| w["address"] == address)
                    .ok_or_else(|| {
                        anyhow::anyhow!("That window has closed; select another instance")
                    })?
            };
            if !self.demo {
                crate::windows::focus(target)?;
            }
            if self.settings["history"]["enabled"] == true {
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
                    if self.settings["history"]["enabled"] == true {
                        store.record(id, mode, result.is_ok())?;
                    }
                }
                result?;
            }
            return Ok(json!({"kind":"activated","demo":self.demo}));
        }
        let mut req = req.clone();
        if let Some(e) = entry {
            req["entry"] = serde_json::to_value(e)?;
        }
        let result = self.bridge(req)?;
        if result["apply_config"] == true {
            self.settings = config::load()?;
            self.icons = Icons::new(&self.settings);
            self.custom = strings(&self.settings["plugins"]["enabled"])
                .iter()
                .any(|n| {
                    ![
                        "Words",
                        "Clipboard",
                        "Recorder",
                        "Screenshots",
                        "Todo",
                        "Browser",
                    ]
                    .contains(&n.as_str())
                });
            self.refresh(false)?;
            self.reset();
        }
        Ok(result)
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
                if self.settings["history"]["enabled"] == true {
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
            "app-action" | "undo-trash" => self.bridge(req)?,
            _ => bail!("Unknown operation"),
        };
        Ok(Some(result))
    }
}
