//! Notification lifecycle independent of GTK, D-Bus, subprocesses, and wall clocks.
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub enum Effect {
    Backend(Value),
    Closed(u32, u32),
    Action(u32, String),
    Command(Vec<String>),
    Copy(String),
}
pub struct Controller {
    pub config: Value,
    pub notices: Vec<Value>,
    pub history: Vec<Value>,
    pub ready: bool,
    pub paused: bool,
    pub lock: String,
    pub selected: String,
    pub monitors: Vec<Value>,
    pub focused_app: String,
    pub error: String,
    pub effects: Vec<Effect>,
    pub revision: u64,
    pub bridge_until: u64,
    pub idle_seconds: u64,
    pub held: HashSet<String>,
    pub floating: HashSet<String>,
    hovered: HashSet<String>,
    sequence: u64,
    native_sequence: u32,
    session: String,
}
fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn id(n: &Value) -> &str {
    s(&n["id"])
}
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
pub fn telegram(n: &Value) -> bool {
    n["type"] == "telegram"
        || [s(&n["app"]), s(&n["desktopEntry"])].iter().any(|v| {
            [
                "telegram",
                "telegram desktop",
                "org.telegram.desktop",
                "telegram via notjik",
            ]
            .contains(&v.to_lowercase().as_str())
        })
}
impl Controller {
    pub fn new(config: Value, session: String) -> Self {
        Self {
            config,
            session,
            notices: vec![],
            history: vec![],
            ready: false,
            paused: false,
            lock: "unknown".into(),
            selected: String::new(),
            monitors: vec![],
            focused_app: String::new(),
            error: String::new(),
            effects: vec![],
            revision: 0,
            bridge_until: 0,
            idle_seconds: 120,
            held: HashSet::new(),
            floating: HashSet::new(),
            hovered: HashSet::new(),
            sequence: 0,
            native_sequence: 0,
        }
    }
    pub fn locked(&self) -> bool {
        self.lock != "unlocked"
    }
    pub fn concealed(&self) -> bool {
        !self.ready
            || self.paused
            || (self.locked() && self.config["privacy"]["lockScreen"] != "show")
    }
    pub fn blocked(&self, n: &Value) -> bool {
        self.config["actions"]["blacklist"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|b| {
                [s(&n["app"]), s(&n["desktopEntry"])]
                    .iter()
                    .any(|v| v.trim().eq_ignore_ascii_case(s(b).trim()))
            })
    }
    pub fn allowed(&self, n: &Value) -> bool {
        self.ready && !self.locked() && !self.paused && !self.blocked(n) && n["archived"] != true
    }
    pub fn find(&self, key: &str) -> Option<&Value> {
        self.notices.iter().find(|n| id(n) == key)
    }
    pub fn snapshot(n: &Value) -> Value {
        let mut n = n.clone();
        if let Some(o) = n.as_object_mut() {
            o.remove("remaining");
            o.remove("requestedTimeout");
            o.remove("nativeId");
        }
        n["actions"] = json!(n["actions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| json!({"label":a["label"]}))
            .collect::<Vec<_>>());
        n
    }
    fn log(&mut self, event: &str, n: &Value) {
        self.effects.push(Effect::Backend(
            json!({"event":event,"session":self.session,"notice":Self::snapshot(n)}),
        ));
    }
    pub fn update(&mut self, event: Value) {
        if let Some(idle) = event["idleSeconds"].as_u64() {
            self.idle_seconds = idle;
        }
        if event["config"].is_object() {
            self.config = event["config"].clone();
            self.ready = true;
            self.error.clear();
        }
        if let Some(v) = event["paused"].as_bool() {
            self.paused = v;
        }
        if let Some(v) = event["lockState"].as_str() {
            self.lock = v.into();
        }
        if let Some(items) = event["history"].as_array() {
            self.history = items.clone();
        }
        if let Some(e) = event["error"].as_str() {
            self.error = e.into();
        }
        self.revision += 1;
    }
    pub fn disconnected(&mut self) {
        self.ready = false;
        self.lock = "unknown".into();
        self.error = "Backend disconnected".into();
        self.idle_seconds = 120;
        self.revision += 1;
    }
    fn timeout(&self, value: Option<f64>, urgency: &str) -> f64 {
        value
            .filter(|v| *v >= 0.)
            .unwrap_or_else(|| self.config["timeouts"][urgency].as_f64().unwrap_or(8.) * 1000.)
    }
    pub fn send(&mut self, input: Value, now: u64) -> Result<String> {
        validate(&input)?;
        let kind = input["type"].as_str().unwrap_or("standard");
        // Native custom types have an explicit standard-card fallback.
        ensure!(
            [
                "standard",
                "progress",
                "status",
                "timer",
                "telegram",
                "results-table",
                "todo",
                "auth-code",
                "key-value"
            ]
            .contains(&kind)
                || self.config["components"][kind].is_string(),
            "Unknown component: {kind}"
        );
        let app = input["app"].as_str().unwrap_or("notjikctl");
        let sender = s(&input["id"]);
        let key = if !sender.is_empty() {
            format!("named-{}/{}", encode(app), encode(sender))
        } else if let Some(replace) = input["replace"].as_str() {
            ensure!(
                self.find(replace).is_some_and(|n| n["nativeId"].is_null()),
                "--replace requires an existing local notification ID"
            );
            replace.into()
        } else {
            self.sequence += 1;
            format!("local-{}", self.sequence)
        };
        let mut n = input.clone();
        n["id"] = json!(key);
        n["senderId"] = json!(sender);
        n["app"] = json!(app);
        n["type"] = json!(kind);
        for (k, v) in [
            ("body", json!("")),
            ("urgency", json!("normal")),
            ("data", json!({})),
            ("actions", json!([])),
            ("icon", json!("")),
            ("image", json!("")),
            ("group", json!("")),
        ] {
            if n[k].is_null() {
                n[k] = v;
            }
        }
        n["received"] = self
            .find(&key)
            .map(|n| n["received"].clone())
            .unwrap_or(json!(now));
        n["updatedAt"] = json!(now);
        let lifetime = if !sender.is_empty() && input["timeout"].is_null() {
            Some(0.)
        } else {
            input["timeout"].as_f64()
        };
        n["remaining"] = json!(self.timeout(lifetime, s(&n["urgency"])));
        self.put(n, now)?;
        Ok(key)
    }
    pub fn notify(&mut self, mut n: Value, replace: u32, now: u64) -> Result<u32> {
        let existing = replace != 0 && self.find(&format!("dbus-{replace}")).is_some();
        let number = if existing {
            replace
        } else {
            loop {
                self.native_sequence = self.native_sequence.wrapping_add(1).max(1);
                if self
                    .find(&format!("dbus-{}", self.native_sequence))
                    .is_none()
                {
                    break self.native_sequence;
                }
            }
        };
        n["id"] = json!(format!("dbus-{number}"));
        n["nativeId"] = json!(number);
        n["received"] = json!(now);
        n["updatedAt"] = json!(now);
        n["remaining"] = json!(self.timeout(n["requestedTimeout"].as_f64(), s(&n["urgency"])));
        if telegram(&n)
            && n["category"] == "im.received"
            && self.bridge_until > now
            && !self.locked()
            && !self.paused
        {
            self.log("suppressed", &n);
            if existing {
                self.close(&format!("dbus-{number}"), 1);
            } else {
                self.effects.push(Effect::Closed(number, 1));
            }
            return Ok(number);
        }
        if self.put(n, now).is_err() {
            self.effects.push(Effect::Closed(number, 2));
        }
        Ok(number)
    }
    fn put(&mut self, mut n: Value, _now: u64) -> Result<()> {
        let focused = self.focused_app.to_lowercase();
        if telegram(&n)
            && (focused.starts_with("org.telegram.desktop") || focused.starts_with("telegram"))
        {
            self.log("suppressed", &n);
            if self.find(id(&n)).is_some() {
                self.close(id(&n), 1);
            } else if let Some(native) = n["nativeId"].as_u64() {
                self.effects.push(Effect::Closed(native as u32, 1));
            }
            return Ok(());
        }
        let existing = self.notices.iter().position(|v| id(v) == id(&n));
        n["robotIndex"] = existing
            .map(|i| self.notices[i]["robotIndex"].clone())
            .unwrap_or(json!(self.sequence + self.native_sequence as u64));
        if let Some(i) = existing {
            self.notices[i] = n.clone();
            self.log("updated", &n);
        } else {
            while self.notices.len() >= self.config["maxQueued"].as_u64().unwrap_or(100) as usize {
                let candidate = self
                    .notices
                    .iter()
                    .find(|v| {
                        v["remaining"].as_f64().unwrap_or(0.) > 0. && !self.held.contains(id(v))
                    })
                    .map(|n| id(n).to_string());
                if let Some(key) = candidate {
                    self.close(&key, 1);
                } else {
                    self.log("overflow", &n);
                    bail!("Queue is full of persistent notices");
                }
            }
            self.notices.push(n.clone());
            self.log("received", &n);
        }
        self.revision += 1;
        Ok(())
    }
    pub fn close(&mut self, key: &str, reason: u32) {
        if let Some(i) = self.notices.iter().position(|n| id(n) == key) {
            let n = self.notices.remove(i);
            self.log(
                match reason {
                    1 => "expired",
                    2 => "dismissed",
                    _ => "closed",
                },
                &n,
            );
            if let Some(native) = n["nativeId"].as_u64() {
                self.effects.push(Effect::Closed(native as u32, reason));
            }
            self.held.remove(key);
            self.floating.remove(key);
            self.hovered.remove(key);
            if self.selected == key {
                self.selected.clear();
            }
            self.revision += 1;
        }
    }
    pub fn dismiss(&mut self, key: &str, shown: &HashSet<String>) {
        if !self.locked() && !self.paused && (shown.contains(key) || self.floating.contains(key)) {
            if let Some(n) = self.find(key).cloned() {
                self.log("user-dismissed", &n);
            }
        }
        self.close(key, 2);
    }
    pub fn hover(&mut self, key: &str, on: bool) {
        if on {
            self.hovered.insert(key.into());
        } else {
            self.hovered.remove(key);
        }
    }
    pub fn hold(&mut self, key: &str, on: bool) {
        if on {
            self.held.insert(key.into());
        } else {
            self.held.remove(key);
        }
    }
    pub fn visible(&self) -> Vec<Value> {
        if self.concealed() || self.monitors.is_empty() {
            return vec![];
        }
        let mut groups: Vec<Vec<&Value>> = vec![];
        let mut last: HashMap<String, usize> = HashMap::new();
        let mut ordered = self
            .notices
            .iter()
            .filter(|n| !self.floating.contains(id(n)))
            .collect::<Vec<_>>();
        ordered.sort_by_key(|n| n["received"].as_u64().unwrap_or(0));
        for n in ordered {
            let key = json!([
                n["desktopEntry"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(s(&n["app"])),
                n["group"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(s(&n["senderId"]))
            ])
            .to_string();
            let window = self.config["grouping"]["windowSeconds"]
                .as_u64()
                .unwrap_or(120)
                * 1000;
            let prev = last.get(&key).copied().filter(|i| {
                self.config["grouping"]["enabled"] == true
                    && (window == 0
                        || n["received"].as_u64().unwrap_or(0).saturating_sub(
                            groups[*i].last().unwrap()["received"].as_u64().unwrap_or(0),
                        ) <= window)
            });
            if let Some(i) = prev {
                groups[i].push(n);
            } else {
                last.insert(key, groups.len());
                groups.push(vec![n]);
            }
        }
        let mut values = groups
            .iter()
            .map(|g| {
                let mut n = (*g.iter().find(|n| id(n) == self.selected).unwrap_or(&g[0])).clone();
                n["groupCount"] = json!(g.len());
                n["memberIds"] = json!(g.iter().map(|n| id(n)).collect::<Vec<_>>());
                n
            })
            .collect::<Vec<_>>();
        let selected = values.iter().find(|n| id(n) == self.selected).cloned();
        values.truncate(self.config["maxVisible"].as_u64().unwrap_or(4).min(4) as usize);
        if let Some(n) = selected {
            if !values.iter().any(|v| id(v) == id(&n)) {
                if let Some(last) = values.last_mut() {
                    *last = n;
                }
            }
        }
        values.reverse();
        values
    }
    pub fn tick(&mut self, elapsed: u64) {
        if self.concealed() || self.locked() || self.monitors.is_empty() {
            return;
        }
        let mut expired = vec![];
        for n in &mut self.notices {
            let key = id(n);
            if self.floating.contains(key)
                || self.held.contains(key)
                || key == self.selected
                || (self.config["pauseOnHover"] == true && self.hovered.contains(key))
            {
                continue;
            }
            let remaining = n["remaining"].as_f64().unwrap_or(0.);
            if remaining > 0. {
                let next = remaining - elapsed.min(1000) as f64;
                n["remaining"] = json!(next);
                if next <= 0. {
                    expired.push(id(n).to_string());
                }
            }
        }
        for key in expired {
            self.close(&key, 1);
        }
    }
    pub fn invoke(&mut self, key: &str, index: usize) -> Result<()> {
        let n = self
            .find(key)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Notification not found"))?;
        ensure!(self.allowed(&n), "Actions unavailable");
        let a = n["actions"]
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("Action not found"))?;
        self.log("action", &n);
        if let Some(native) = n["nativeId"].as_u64() {
            self.effects
                .push(Effect::Action(native as u32, s(&a["identifier"]).into()));
            for hook in self.config["actions"]["nativeHooks"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if hook["identifier"] == a["identifier"]
                    && [s(&n["app"]), s(&n["desktopEntry"])]
                        .iter()
                        .any(|v| v.eq_ignore_ascii_case(s(&hook["caller"])))
                {
                    self.effects
                        .push(Effect::Command(command(&hook["command"])?));
                }
            }
        } else if let Some(copy) = a["copy"].as_str() {
            self.effects.push(Effect::Copy(copy.into()));
        } else {
            self.effects.push(Effect::Command(command(&a["command"])?));
        }
        if n["resident"] != true {
            self.close(key, 2);
        }
        Ok(())
    }
    pub fn handle(&mut self, r: Value, now: u64) -> Result<Value> {
        ensure!(
            self.ready || r["op"] == "status",
            "Notification backend is not ready"
        );
        let shown = self
            .visible()
            .iter()
            .map(|n| id(n).to_string())
            .collect::<HashSet<_>>();
        match s(&r["op"]) {
            "send" => return Ok(json!({"ok":true,"id":self.send(r["notice"].clone(),now)?})),
            "status" => {}
            "list" => {
                return Ok(
                    json!({"ok":true,"notices":if self.locked(){vec![]}else{self.notices.iter().map(Self::snapshot).collect()}}),
                )
            }
            "monitors" => return Ok(json!({"ok":true,"monitors":self.monitors})),
            "pause" | "resume" | "toggle" => {
                self.paused = match s(&r["op"]) {
                    "pause" => true,
                    "resume" => false,
                    _ => !self.paused,
                };
                self.effects.push(Effect::Backend(
                    json!({"op":"pausePreference","paused":self.paused}),
                ));
                self.revision += 1;
            }
            "dismiss" => {
                let key = s(&r["id"]);
                ensure!(
                    key == "all" || self.find(key).is_some(),
                    "Notification not found"
                );
                let ids = self
                    .notices
                    .iter()
                    .filter(|n| key == "all" || id(n) == key)
                    .map(|n| id(n).to_string())
                    .collect::<Vec<_>>();
                for key in ids {
                    self.dismiss(&key, &shown);
                }
            }
            "global-remove" | "telegramRemove" => {
                let key = s(&r["id"]);
                if r["op"] == "global-remove"
                    || self
                        .find(key)
                        .is_some_and(|n| n["app"] == "Telegram via Notjik")
                {
                    self.close(key, 3);
                }
            }
            "openTelegramChat" => {
                let n = self
                    .find(s(&r["id"]))
                    .context("Notification not found")?
                    .clone();
                ensure!(
                    telegram(&n) && n["nativeId"].is_number(),
                    "Native Telegram chat unavailable"
                );
                let index = n["actions"]
                    .as_array()
                    .and_then(|a| a.iter().position(|v| v["identifier"] == "default"))
                    .context("Default action unavailable")?;
                self.invoke(id(&n), index)?;
            }
            "action" => {
                self.invoke(
                    s(&r["id"]),
                    r["index"]
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("Invalid action index"))?
                        as usize,
                )?;
            }
            "telegramBridge" => {
                self.bridge_until = if r["active"] == true { now + 12000 } else { 0 }
            }
            "unfocus" => {
                self.selected.clear();
                self.revision += 1;
            }
            "navigate" => {
                ensure!(!self.locked(), "Locked");
                ensure!(
                    ["previous", "next"].contains(&s(&r["direction"])),
                    "Invalid direction"
                );
                let mut items = self.notices.iter().collect::<Vec<_>>();
                items.reverse();
                if !items.is_empty() {
                    let current = items.iter().position(|n| id(n) == self.selected);
                    let index = match current {
                        None => 0,
                        Some(i) => {
                            if r["direction"] == "previous" {
                                (i + 1) % items.len()
                            } else {
                                (i + items.len() - 1) % items.len()
                            }
                        }
                    };
                    self.selected = id(items[index]).into();
                    self.revision += 1;
                }
            }
            "selected" => {
                ensure!(!self.locked(), "Locked");
                let n = self
                    .find(&self.selected)
                    .cloned()
                    .or_else(|| self.visible().first().cloned());
                if let Some(n) = n {
                    if r["action"] == "dismiss" {
                        self.dismiss(id(&n), &shown);
                    } else {
                        let actions = n["actions"].as_array().cloned().unwrap_or_default();
                        let primary = actions
                            .iter()
                            .position(|a| a["identifier"] == "default")
                            .unwrap_or(0);
                        let index = if r["action"] == "secondary" {
                            (0..actions.len()).find(|i| *i != primary)
                        } else {
                            Some(primary)
                        };
                        if let Some(i) = index {
                            self.invoke(id(&n), i)?;
                        }
                    }
                }
            }
            "history-data" => {
                return Ok(
                    json!({"ok":true,"items":if self.locked(){vec![]}else{self.history.clone()},"paused":self.paused,"locked":self.locked(),"avatars":{},"config":self.config}),
                )
            }
            "history" => bail!("Control Center is provided by the BarTjik panel"),
            "history-action" => {
                ensure!(!self.locked(), "Locked");
                let n = self
                    .history
                    .iter()
                    .find(|n| n["historyKey"] == r["key"])
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("History entry not found"))?;
                ensure!(
                    n["session"] == self.session && self.find(id(&n)).is_some(),
                    "Notification is no longer active"
                );
                if r["action"] == "invoke" {
                    self.invoke(id(&n), r["index"].as_u64().unwrap_or(0) as usize)?;
                } else {
                    self.close(id(&n), 2);
                }
            }
            "reload" => self.effects.push(Effect::Backend(json!({"op":"history"}))),
            _ => bail!("Unknown operation"),
        }
        Ok(
            json!({"ok":true,"renderer":"gtk","paused":self.paused,"queued":self.notices.len(),"visible":self.visible().len(),"lockState":self.lock,"selectedId":self.selected,"keyboardMode":!self.selected.is_empty(),"theme":self.config["theme"],"groupingEnabled":self.config["grouping"]["enabled"],"telegramBridgeActive":self.bridge_until>now,"focusedAppId":self.focused_app,"historyCount":self.history.len(),"historyError":self.error,"drawerOpen":false,"floating":self.floating.len(),"pinned":self.floating.len(),"monitors":self.monitors,"idle":self.idle_seconds >= 120,"idleSeconds":self.idle_seconds}),
        )
    }
}
/// Presentation adapters never alter canonical identity, actions or history.
pub fn presentation(notice: &Value, config: &Value) -> Value {
    let mut result = notice.clone();
    if !["standard", "telegram"].contains(&notice["type"].as_str().unwrap_or("standard"))
        || (notice["type"] == "telegram" && notice["data"]["kind"] != "text")
    {
        return result;
    }
    let body = s(&notice["body"]).replace("\r\n", "\n").replace('\r', "\n");
    if body.is_empty() || body.len() > 16384 {
        return result;
    }
    for rule in config["handlers"].as_array().into_iter().flatten() {
        let m = &rule["match"];
        if (m["telegram"] == true && !telegram(notice))
            || (m["app"].is_string()
                && ![s(&notice["app"]), s(&notice["desktopEntry"])]
                    .iter()
                    .any(|v| v.eq_ignore_ascii_case(s(&m["app"]))))
            || (m["title"].is_string() && m["title"] != notice["title"])
            || (m["chatId"].is_string() && m["chatId"] != notice["data"]["chatId"])
        {
            continue;
        }
        let Ok(pattern) = fancy_regex::RegexBuilder::new(&format!("^(?:{})$", s(&rule["pattern"])))
            .backtrack_limit(100_000)
            .build()
        else {
            continue;
        };
        let Ok(Some(captures)) = pattern.captures(body.trim()) else {
            continue;
        };
        let mut data = json!({});
        let mut complete = true;
        for (key, index) in rule["captures"].as_object().into_iter().flatten() {
            let value = captures
                .get(index.as_u64().unwrap_or(0) as usize)
                .map(|v| v.as_str().trim())
                .unwrap_or("");
            if value.is_empty() {
                complete = false;
                break;
            }
            data[key] = json!(value);
        }
        if !complete {
            continue;
        }
        data["fields"] = json!(rule["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| json!({"label":f["label"],"value":data[s(&f["key"])]}))
            .collect::<Vec<_>>());
        result["type"] = rule["card"].clone();
        result["data"] = data;
        return result;
    }
    result
}

pub fn command(v: &Value) -> Result<Vec<String>> {
    let args = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Command must be an argument array"))?;
    ensure!(
        !args.is_empty() && args.len() <= 64,
        "Invalid command length"
    );
    let result = args
        .iter()
        .map(|v| {
            let s = v
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid argument"))?;
            ensure!(!s.contains('\0') && s.len() <= 8192, "Invalid argument");
            Ok(s.to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(!result[0].is_empty(), "Missing executable");
    Ok(result)
}
pub fn validate(n: &Value) -> Result<()> {
    ensure!(n.is_object(), "Notice must be an object");
    ensure!(
        n["title"]
            .as_str()
            .is_some_and(|s| s.chars().count() <= 4096),
        "Invalid title"
    );
    for k in ["body", "app", "type", "icon", "image", "desktopEntry"] {
        ensure!(
            n[k].is_null()
                || n[k]
                    .as_str()
                    .is_some_and(|s| s.len() <= 65536 && !s.contains('\0')),
            "Invalid {k}"
        );
    }
    if !n["id"].is_null() {
        ensure!(
            n["id"].as_str().is_some_and(|s| regex::Regex::new(
                r"^[a-zA-Z0-9][a-zA-Z0-9_.:/-]{0,127}$"
            )
            .unwrap()
            .is_match(s)),
            "Invalid id"
        );
        ensure!(n["replace"].is_null(), "id and replace cannot be combined");
    }
    ensure!(
        n["urgency"].is_null() || ["low", "normal", "critical"].contains(&s(&n["urgency"])),
        "Invalid urgency"
    );
    ensure!(
        n["group"].is_null()
            || n["group"]
                .as_str()
                .is_some_and(|s| s.chars().count() <= 256),
        "Invalid group"
    );
    ensure!(
        n["timeout"].is_null()
            || n["timeout"]
                .as_f64()
                .is_some_and(|v| (-1. ..=604800000.).contains(&v)),
        "Invalid timeout"
    );
    ensure!(n["data"].is_null() || n["data"].is_object(), "Invalid data");
    if !n["actions"].is_null() {
        let actions = n["actions"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid actions"))?;
        ensure!(actions.len() <= 16, "Too many actions");
        for a in actions {
            ensure!(
                a["label"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty() && s.chars().count() <= 120),
                "Invalid action label"
            );
            if let Some(copy) = a["copy"].as_str() {
                ensure!(
                    regex::Regex::new(r"^[A-Za-z0-9 -]{4,32}$")?.is_match(copy)
                        && a["command"].is_null(),
                    "Invalid copy action"
                );
            } else {
                command(&a["command"])?;
            }
        }
    }
    if n["type"] == "progress" {
        ensure!(
            n["data"]["value"]
                .as_f64()
                .is_some_and(|v| (0. ..=100.).contains(&v)),
            "Invalid progress"
        );
    }
    if n["type"] == "timer" {
        ensure!(
            n["data"]["seconds"]
                .as_f64()
                .is_some_and(|v| (0. ..=604800.).contains(&v)),
            "Invalid timer"
        );
    }
    if n["type"] == "telegram" {
        let d = &n["data"];
        ensure!(
            ["text", "photo", "voice", "sticker", "emoji", "animation"].contains(&s(&d["kind"])),
            "Invalid Telegram kind"
        );
        for k in ["chatId", "messageId"] {
            crate::telegram::identifier(&d[k])?;
        }
        ensure!(
            d["source"].is_null()
                || s(&d["source"]).is_empty()
                || s(&d["source"]).starts_with("file:///"),
            "Media must be local"
        );
        if let Some(parts) = d["parts"].as_array() {
            ensure!(parts.len() <= 4096, "Too many text parts");
            for p in parts {
                ensure!(
                    p["source"].is_null()
                        || s(&p["source"]).is_empty()
                        || s(&p["source"]).starts_with("file:///"),
                    "Emoji must be local"
                );
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn controller() -> Controller {
        let mut c = Controller::new(crate::config::defaults(), "test".into());
        c.ready = true;
        c.lock = "unlocked".into();
        c.monitors = vec![json!({"name":"test"})];
        c
    }
    #[test]
    fn replacements_and_close_reasons() {
        let mut c = controller();
        let a = c.send(json!({"title":"A","id":"fixed"}), 1).unwrap();
        assert_eq!(c.send(json!({"title":"B","id":"fixed"}), 2).unwrap(), a);
        assert_eq!(c.notices.len(), 1);
        assert_eq!(c.notices[0]["received"], 1);
        assert_eq!(c.notices[0]["remaining"], 0.);
        let n = json!({"title":"native","urgency":"normal","requestedTimeout":0});
        let id = c.notify(n.clone(), 0, 3).unwrap();
        assert_eq!(c.notify(n, id, 4).unwrap(), id);
        c.close(&format!("dbus-{id}"), 3);
        assert!(c.effects.iter().any(|e| matches!(e, Effect::Closed(_, 3))));
    }
    #[test]
    fn suppressed_replacements_remove_previous_card_once() {
        for bridge in [false, true] {
            let mut c = controller();
            let n = json!({"title":"Telegram", "app":"Telegram", "category":"im.received"});
            let native = c.notify(n.clone(), 0, 1).unwrap();
            c.effects.clear();
            if bridge {
                c.bridge_until = 100;
            } else {
                c.focused_app = "org.telegram.desktop".into();
            }
            c.notify(n, native, 2).unwrap();
            assert!(c.notices.is_empty());
            assert_eq!(
                c.effects
                    .iter()
                    .filter(|e| matches!(e, Effect::Closed(_, 1)))
                    .count(),
                1
            );
        }
    }
    #[test]
    fn disconnected_status_remains_conservative() {
        let mut c = controller();
        c.idle_seconds = 0;
        c.disconnected();
        let status = c.handle(json!({"op":"status"}), 1).unwrap();
        assert_eq!(status["lockState"], "unknown");
        assert_eq!(status["idle"], true);
        assert!(c
            .handle(json!({"op":"send","notice":{"title":"hidden"}}), 1)
            .is_err());
    }
    #[test]
    fn privacy_holds_and_expiry() {
        let mut c = controller();
        let id = c.send(json!({"title":"A","timeout":200}), 0).unwrap();
        c.lock = "unknown".into();
        c.tick(500);
        assert!(c.visible().is_empty());
        assert!(c.find(&id).is_some());
        c.lock = "unlocked".into();
        c.hover(&id, true);
        c.tick(500);
        assert!(c.find(&id).is_some());
        c.hover(&id, false);
        c.tick(201);
        assert!(c.find(&id).is_none());
    }
    #[test]
    fn releasing_interaction_hold_preserves_floating_card() {
        let mut c = controller();
        let id = c
            .send(json!({"title":"Floating","timeout":200}), 0)
            .unwrap();
        c.hold(&id, true);
        c.floating.insert(id.clone());
        c.hold(&id, false);
        c.tick(500);
        assert!(c.floating.contains(&id));
        assert!(!c.held.contains(&id));
        assert!(c.find(&id).is_some());
        assert!(c.visible().is_empty());
        c.close(&id, 2);
        assert!(!c.floating.contains(&id));
    }
    #[test]
    fn grouped_oldest_and_selected() {
        let mut c = controller();
        let a = c.send(json!({"title":"A"}), 1).unwrap();
        let b = c.send(json!({"title":"B"}), 2).unwrap();
        assert_eq!(c.visible()[0]["id"], a);
        assert_eq!(c.visible()[0]["groupCount"], 2);
        c.selected = b.clone();
        assert_eq!(c.visible()[0]["id"], b);
    }
    #[test]
    fn overflow_and_blacklist_do_not_execute() {
        let mut c = controller();
        c.config["maxQueued"] = json!(1);
        let a = c
            .send(
                json!({"title":"A","timeout":0,"actions":[{"label":"Go","command":["true"]}]}),
                0,
            )
            .unwrap();
        assert!(c.send(json!({"title":"B"}), 0).is_err());
        c.config["actions"]["blacklist"] = json!(["notjikctl"]);
        assert!(c.invoke(&a, 0).is_err());
        assert!(!c.effects.iter().any(|e| matches!(e, Effect::Command(_))));
    }
    #[test]
    fn native_actions_keep_identifiers_and_residency() {
        let mut c = controller();
        let id=c.notify(json!({"title":"A","resident":true,"actions":[{"identifier":"open-file","label":"Open"}]}),0,1).unwrap();
        c.invoke(&format!("dbus-{id}"), 0).unwrap();
        assert!(c.find(&format!("dbus-{id}")).is_some());
        assert!(c
            .effects
            .iter()
            .any(|e| matches!(e,Effect::Action(_,s) if s=="open-file")));
    }
}
