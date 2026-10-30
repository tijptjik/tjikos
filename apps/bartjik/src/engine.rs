use crate::{actions, battery, cache, config, demo, lyrics, providers, util::*, workspaces};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    sync::{mpsc, Mutex, Notify},
    task::JoinHandle,
};
enum Event {
    Provider(String, Value),
    Compositor(Value, String),
    Emit(Value),
    Action(String, Value),
}
struct Task(JoinHandle<()>);
impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct Worker {
    _task: Task,
    wake: Arc<Notify>,
    state: Value,
}
struct ProcessGroup(tokio::process::Child);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if let Some(pid) = self.0.id() {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}
async fn stream(
    spec: &Value,
    key: &str,
    tx: &mpsc::Sender<Event>,
    polls: u64,
    last: &mut Vec<Value>,
    updated: &mut Value,
    failures: &mut u32,
) -> Result<()> {
    let args = config::argv(&spec["command"])?;
    let mut p = ProcessGroup(
        tokio::process::Command::new(&args[0])
            .args(&args[1..])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .kill_on_drop(true)
            .spawn()?,
    );
    let mut reader = BufReader::new(p.0.stdout.take().unwrap());
    let limit = spec["maxOutput"].as_u64().unwrap_or(262144);
    loop {
        let mut raw = vec![];
        tokio::time::timeout(
            Duration::from_secs_f64(spec["timeout"].as_f64().unwrap_or(30.)),
            (&mut reader).take(limit + 1).read_until(b'\n', &mut raw),
        )
        .await
        .context("stream heartbeat timed out")??;
        ensure!(!raw.is_empty(), "stream exited; reconnecting");
        ensure!(raw.len() as u64 <= limit, "stream output limit exceeded");
        let rows = providers::untrusted(serde_json::from_slice(&raw)?)?;
        *last = rows.clone();
        *updated = json!(now());
        *failures = 0;
        tx.send(Event::Provider(key.into(),json!({"items":rows,"error":"","updated":now(),"polls":polls,"failures":0,"state":"ready"}))).await?;
    }
}
fn worker(
    key: String,
    spec: Value,
    tx: mpsc::Sender<Event>,
    alert: Arc<Mutex<battery::Alert>>,
) -> Worker {
    let wake = Arc::new(Notify::new());
    let w = wake.clone();
    let task = tokio::spawn(async move {
        let mut polls = 0u64;
        let mut failures = 0u32;
        let mut rows = vec![];
        let mut updated = Value::Null;
        loop {
            polls += 1;
            let mut delay = spec["interval"]
                .as_f64()
                .or(config::contract()["INTERVALS"][s(&spec["provider"])].as_f64())
                .unwrap_or(5.);
            let result = if spec["provider"] == "process" && spec["mode"] == "stream" {
                stream(
                    &spec,
                    &key,
                    &tx,
                    polls,
                    &mut rows,
                    &mut updated,
                    &mut failures,
                )
                .await
                .map(|_| vec![])
            } else {
                providers::poll(&spec).await
            };
            let state = match result.and_then(|r| {
                providers::validate(&r)?;
                Ok(r)
            }) {
                Ok(r) => {
                    rows = r;
                    updated = json!(now());
                    failures = 0;
                    if spec["provider"] == "battery" {
                        alert.lock().await.update(&rows).await;
                    }
                    json!({"items":rows,"error":"","updated":updated,"polls":polls,"failures":0,"state":"ready"})
                }
                Err(e) => {
                    failures += 1;
                    delay = delay.max(2f64.powi(failures.min(8) as i32).min(300.));
                    let raw = e.to_string();
                    if let Some(d) = raw
                        .strip_prefix("HTTP 429; retry ")
                        .and_then(|v| v.parse::<f64>().ok())
                    {
                        delay = delay.max(d);
                    }
                    let error = if raw.starts_with("HTTP ")
                        || raw.contains("unavailable")
                        || raw.contains("timed out")
                        || raw.contains("limit")
                        || raw.contains("not set")
                        || raw.contains("No ")
                    {
                        raw
                    } else {
                        "Invalid provider response".into()
                    };
                    let mut stale = rows.clone();
                    if stale.is_empty() {
                        stale.push(item(
                            "unavailable",
                            Some("—"),
                            None,
                            "glyph",
                            json!({"unavailable":true}),
                            &error,
                            json!({}),
                        ));
                    }
                    for r in &mut stale {
                        r["state"]["stale"] = json!(!updated.is_null());
                        r["tooltip"] = json!(format!("{}\n{error}", s(&r["tooltip"])).trim());
                    }
                    json!({"items":stale,"error":error,"updated":updated,"polls":polls,"failures":failures,"state":if updated.is_null(){"unavailable"}else{"stale"}})
                }
            };
            if tx.send(Event::Provider(key.clone(), state)).await.is_err() {
                break;
            }
            tokio::select! {_=tokio::time::sleep(Duration::from_secs_f64(delay))=>{},_=w.notified()=>{}}
        }
    });
    Worker {
        _task: Task(task),
        wake,
        state: json!({"items":[],"error":"starting","updated":null,"polls":0,"failures":0,"state":"unavailable"}),
    }
}
fn pointer(tx: mpsc::Sender<Event>) -> Task {
    Task(tokio::spawn(async move {
        let mut monitors = json!([]);
        let mut refresh = 0.;
        loop {
            let result = async {
                if now() >= refresh {
                    monitors = workspaces::query("monitors").await?;
                    refresh = now() + 1.;
                }
                workspaces::local_position(&workspaces::query("cursorpos").await?, &monitors)
            }
            .await;
            let mut delay = 0.05;
            let point = result.unwrap_or_else(|_: anyhow::Error| {
                refresh = 0.;
                delay = 1.;
                json!({"valid":false,"error":"Pointer observer unavailable; auto-hide suspended"})
            });
            if tx
                .send(Event::Emit(json!({"kind":"pointer","pointer":point})))
                .await
                .is_err()
            {
                break;
            }
            tokio::time::sleep(Duration::from_secs_f64(delay)).await;
        }
    }))
}
fn watcher(tx: mpsc::Sender<Event>, geometry: bool) -> Task {
    Task(tokio::spawn(async move {
        let mut data = json!({});
        let mut backoff = 1;
        loop {
            let run = async {
                let path = std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR")?)
                    .join("hypr")
                    .join(std::env::var("HYPRLAND_INSTANCE_SIGNATURE")?)
                    .join(".socket2.sock");
                let socket = tokio::net::UnixStream::connect(path).await?;
                let mut reader = BufReader::new(socket);
                loop {
                    data = workspaces::snapshot().await?;
                    tx.send(Event::Compositor(data.clone(), String::new()))
                        .await?;
                    backoff = 1;
                    let mut raw = vec![];
                    match tokio::time::timeout(
                        Duration::from_millis(if geometry { 500 } else { 10000 }),
                        (&mut reader).take(65537).read_until(b'\n', &mut raw),
                    )
                    .await
                    {
                        Ok(v) => {
                            v?;
                            ensure!(
                                !raw.is_empty() && raw.len() <= 65536,
                                "compositor disconnected"
                            );
                            tokio::time::sleep(Duration::from_millis(60)).await;
                            for _ in 0..100 {
                                let mut burst = vec![];
                                match tokio::time::timeout(
                                    Duration::from_millis(1),
                                    (&mut reader).take(65537).read_until(b'\n', &mut burst),
                                )
                                .await
                                {
                                    Ok(v) => {
                                        v?;
                                        ensure!(
                                            !burst.is_empty() && burst.len() <= 65536,
                                            "compositor disconnected"
                                        );
                                    }
                                    Err(_) => break,
                                }
                            }
                        }
                        Err(_) => {}
                    }
                }
            }
            .await;
            if let Err(_e) = run as Result<()> {
                if tx
                    .send(Event::Compositor(
                        data.clone(),
                        "compositor unavailable; reconnecting".into(),
                    ))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_secs(backoff)).await;
            backoff = (backoff * 2).min(30);
        }
    }))
}
struct Engine {
    demo: bool,
    cfg: Option<Value>,
    bars: Vec<Value>,
    screens: Value,
    workers: HashMap<String, Worker>,
    watcher: Option<Task>,
    geometry: bool,
    pointer: Option<Task>,
    compositor: Value,
    compositor_error: String,
    error: String,
    action_error: String,
    feedback: Value,
    serial: String,
    tx: mpsc::Sender<Event>,
    alert: Arc<Mutex<battery::Alert>>,
    panels: Arc<actions::Panels>,
    artwork: Arc<cache::Artwork>,
    history: Arc<cache::History>,
    lyrics: Arc<lyrics::Lyrics>,
    tasks: Vec<Task>,
}
impl Engine {
    fn publish(&self) {
        let Some(cfg) = &self.cfg else {
            crate::emit(&json!({"error":self.error,"ready":false}));
            return;
        };
        let mut states: serde_json::Map<String, Value> = self
            .workers
            .iter()
            .map(|(k, w)| (k.clone(), w.state.clone()))
            .collect();
        for b in &self.bars {
            for (mid, m) in obj(&b["moduleConfigs"]) {
                let key = s(&b["sources"][&mid]).to_owned();
                if self.demo {
                    states.insert(key,json!({"items":demo::items(s(&m["provider"]),&m),"state":"demo","error":""}));
                } else if m["provider"] == "workspaces" {
                    let mut rows = workspaces::view(&self.compositor, s(&b["screen"]), &m);
                    if !self.compositor_error.is_empty() {
                        for r in &mut rows {
                            r["state"]["stale"] = json!(true);
                        }
                    }
                    states.insert(key,json!({"state":if self.compositor_error.is_empty(){"ready"}else if rows.is_empty(){"unavailable"}else{"stale"},"items":rows,"error":self.compositor_error}));
                }
            }
        }
        crate::emit(
            &json!({"ready":true,"config":cfg,"bars":self.bars,"providers":states,"error":self.error,"actionError":self.action_error,"panelFeedback":self.feedback,"demo":self.demo,"compositor":self.compositor}),
        );
    }
    async fn reload(&mut self, force: bool) -> Result<bool> {
        let cfg = config::load()?;
        let serial = json!([cfg, self.screens]).to_string();
        if !force && serial == self.serial {
            let changed = !self.error.is_empty();
            self.error.clear();
            return Ok(changed);
        }
        let mut bars = vec![];
        for (i, s) in arr(&self.screens).iter().enumerate() {
            bars.extend(config::resolve(&cfg, s, i, false)?);
        }
        let mut needed = HashMap::new();
        let mut need_ws = false;
        let mut geometry = false;
        let mut need_pointer = false;
        for b in &mut bars {
            let used: std::collections::HashSet<String> = arr(&b["islands"])
                .iter()
                .flat_map(|i| arr(&i["modules"]).iter().map(st))
                .collect();
            let modules: serde_json::Map<_, _> = obj(&b["moduleConfigs"])
                .into_iter()
                .filter(|(id, _)| used.contains(id))
                .collect();
            b["moduleConfigs"] = json!(modules);
            b["sources"] = json!({});
            need_pointer |= truth(&b["autoHide"]);
            for (mid, m) in modules {
                let mut key = config::provider_key(&m);
                if m["provider"] == "workspaces" {
                    key.push('@');
                    key.push_str(s(&b["screen"]));
                    need_ws = true;
                    geometry |= truth(&m["offscreen"]);
                }
                need_pointer |= truth(&m["panel"]["hover"])
                    || (m["provider"] == "workspaces" && truth(&m["preview"]["enabled"]));
                b["sources"][&mid] = json!(key);
                if !self.demo
                    && !["playerctl", "icontray", "workspaces"].contains(&s(&m["provider"]))
                {
                    needed.insert(key, config::provider_config(&m));
                }
            }
        }
        self.workers.retain(|k, _| needed.contains_key(k));
        for (k, m) in needed {
            if !self.workers.contains_key(&k) {
                self.workers
                    .insert(k.clone(), worker(k, m, self.tx.clone(), self.alert.clone()));
            }
        }
        if self.demo || !need_ws {
            self.watcher = None;
            self.compositor = json!({});
        } else if self.watcher.is_none() || self.geometry != geometry {
            self.watcher = Some(watcher(self.tx.clone(), geometry));
        }
        self.geometry = geometry;
        if need_pointer && self.pointer.is_none() {
            self.pointer = Some(pointer(self.tx.clone()));
        } else if !need_pointer {
            self.pointer = None;
        }
        self.panels.cancel();
        self.panels.close(None).await;
        self.cfg = Some(cfg);
        self.bars = bars;
        self.serial = serial;
        self.error.clear();
        Ok(true)
    }
    fn context(&self, msg: &Value) -> Result<(Value, Value, String)> {
        let c = &msg["context"];
        let b = self
            .bars
            .iter()
            .find(|b| b["key"] == c["barKey"])
            .context("unknown action bar")?;
        ensure!(c["monitor"] == b["screen"], "action monitor mismatch");
        let m = b["moduleConfigs"]
            .get(s(&c["module"]))
            .context("unknown action module")?;
        Ok((b.clone(), m.clone(), st(&b["sources"][s(&c["module"])])))
    }
    async fn message(&mut self, msg: Value) -> Result<bool> {
        let op = st(&msg["op"]);
        if op == "screens" {
            ensure!(
                msg["screens"].is_array() && arr(&msg["screens"]).len() <= 64,
                "invalid screens"
            );
            self.screens = msg["screens"].clone();
            return self.reload(false).await;
        }
        if op == "reload" {
            return self.reload(true).await;
        }
        if op == "externalLeave" {
            if let Ok((_, m, _)) = self.context(&msg) {
                if ["audio", "network"].contains(&s(&m["provider"])) {
                    self.panels.leave(s(&m["provider"]));
                }
            }
            return Ok(false);
        }
        if op == "closeExternalPanels" {
            self.panels.close(None).await;
            return Ok(false);
        }
        self.tasks.retain(|t| !t.0.is_finished());
        if self.tasks.len() >= 32 {
            self.action_error = "action queue full".into();
            return Ok(true);
        }
        let tx = self.tx.clone();
        let demo = self.demo;
        let token = msg["token"].clone();
        let task: JoinHandle<()> = match op.as_str() {
            "artwork" => {
                let cache = self.artwork.clone();
                tokio::spawn(async move {
                    let source = cache.get(s(&msg["url"])).await.unwrap_or_default();
                    let _ = tx
                        .send(Event::Emit(
                            json!({"kind":"artwork","token":token,"source":source}),
                        ))
                        .await;
                })
            }
            "lyrics" => {
                let cache = self.lyrics.clone();
                tokio::spawn(async move {
                    let result = if demo {
                        json!({"status":"ready","text":"Evening light across the water\nQuiet footsteps on the quay\n\nWindows glow along the harbour\nHome is where the lights will be\n\nLet the city settle slowly\nLet the last boat cross the bay\n\nEvery little light a story\nEvery wave will find its way"})
                    } else {
                        cache
                            .get(&msg["track"])
                            .await
                            .unwrap_or(json!({"status":"error","text":""}))
                    };
                    let _ = tx
                        .send(Event::Emit(
                            json!({"kind":"lyrics","token":token,"lyrics":result}),
                        ))
                        .await;
                })
            }
            "crypto" => {
                let (_, m, _) = self.context(&msg)?;
                ensure!(m["provider"] == "crypto", "Invalid chart context");
                let currency = m["currency"].as_str().unwrap_or("USD").to_uppercase();
                let assets = m.get("assets").unwrap_or(&config::contract()["ASSETS"]);
                let asset = arr(assets)
                    .iter()
                    .find(|a| {
                        format!("{}/{}", s(&a["symbol"]).to_uppercase(), currency)
                            == s(&msg["context"]["item"])
                    })
                    .context("Invalid chart asset")?
                    .clone();
                let cache = self.history.clone();
                tokio::spawn(async move {
                    let symbol = s(&asset["symbol"]).to_uppercase();
                    let fixture = if demo {
                        demo::fixture().and_then(|v| {
                            v["markets"][format!("{symbol}/{currency}")]
                                .get("chart")
                                .cloned()
                        })
                    } else {
                        None
                    };
                    let result = if let Some(chart) = fixture {
                        chart
                    } else if demo {
                        let base = match symbol.as_str() {
                            "BTC" => 67432.,
                            "SOL" => 148.,
                            "AR" => 18.4,
                            _ => 100.,
                        };
                        json!({"points":(0..288).map(|i|json!([now()*1000.-(287-i)as f64*300000.,base*(1.+(i as f64/22.).sin()*0.03+i as f64/10000.)])).collect::<Vec<_>>(),"updated":now(),"source":"Synthetic demo","error":"","stale":false})
                    } else {
                        let coin = asset["chartId"].as_str().unwrap_or(match symbol.as_str() {
                            "BTC" => "bitcoin",
                            "SOL" => "solana",
                            "AR" => "arweave",
                            _ => "",
                        });
                        cache.get(coin, &currency.to_lowercase()).await.unwrap_or(
                            json!({"error":"Chart unavailable: check asset chartId and currency"}),
                        )
                    };
                    let _ = tx
                        .send(Event::Emit(
                            json!({"kind":"crypto","token":token,"chart":result}),
                        ))
                        .await;
                })
            }
            "action" | "panelAction" => {
                if demo {
                    return Ok(op == "panelAction" && demo::control(&msg["context"]));
                }
                let (_, m, key) = self.context(&msg)?;
                let context = msg["context"].clone();
                let rows = if m["provider"] == "workspaces" {
                    workspaces::view(&self.compositor, s(&context["monitor"]), &m)
                } else if ["playerctl", "icontray"].contains(&s(&m["provider"])) {
                    vec![json!({"id":context["item"]})]
                } else {
                    self.workers
                        .get(&key)
                        .map(|w| arr(&w.state["items"]).to_vec())
                        .unwrap_or_default()
                };
                let panels = self.panels.clone();
                let wake = self.workers.get(&key).map(|w| w.wake.clone());
                let panel_action = op == "panelAction";
                let mut feedback = json!({});
                if panel_action {
                    ensure!(
                        ["locale", "mullvad", "tailscale", "battery"].contains(&s(&m["provider"])),
                        "invalid panel action"
                    );
                    feedback = json!({"barKey":context["barKey"],"module":context["module"],"busy":true,"message":"","error":false});
                    self.feedback = feedback.clone();
                }
                tokio::spawn(async move {
                    let result = async {
                        if panel_action {
                            let provider = s(&m["provider"]);
                            let _lock =
                                actions::FileLock::acquire(&format!("bartjik-{provider}.lock"))
                                    .await?;
                            let message = match provider {
                                "battery" => {
                                    battery::select(s(&context["action"]), s(&context["selection"]))
                                        .await?
                                }
                                "locale" => {
                                    let group = cmd(&["fcitx5-remote", "--check", "-q"]).await?;
                                    let methods = crate::details::methods(&group);
                                    let selected = s(&context["selection"]);
                                    ensure!(
                                        methods.iter().any(|m| m == selected),
                                        "method no longer in active group"
                                    );
                                    cmd(&["fcitx5-remote", "--check", "-s", selected]).await?;
                                    "Input method selected".into()
                                }
                                _ if context["action"] == "refresh" => "Refreshing".into(),
                                _ => {
                                    crate::connectivity::act(
                                        provider,
                                        s(&context["action"]),
                                        &context["selection"],
                                        &m,
                                    )
                                    .await?
                                }
                            };
                            feedback["busy"] = json!(false);
                            feedback["message"] = json!(message);
                        } else {
                            let row = rows
                                .iter()
                                .find(|r| r["id"] == context["item"])
                                .context("item no longer available")?;
                            let action =
                                actions::choose(&m, row, s(&context["part"]), s(&context["event"]));
                            if s(&action).ends_with(".panel") {
                                panels
                                    .open(s(&action).split('.').next().unwrap(), &context, &m)
                                    .await?;
                            } else {
                                actions::execute(&action, &context, &m, row, &rows).await?;
                            }
                        }
                        Ok::<_, anyhow::Error>(())
                    }
                    .await;
                    let error = if result.is_err() {
                        "This control is currently unavailable".to_owned()
                    } else {
                        String::new()
                    };
                    if result.is_ok() {
                        if let Some(wake) = wake {
                            wake.notify_one();
                        }
                    } else if panel_action {
                        feedback["busy"] = json!(false);
                        feedback["error"] = json!(true);
                        feedback["message"] = json!(error);
                    }
                    let _ = tx.send(Event::Action(error, feedback)).await;
                })
            }
            _ => return Ok(false),
        };
        self.tasks.push(Task(task));
        Ok(op == "panelAction")
    }
}
pub async fn serve(demo: bool) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(256);
    let mut engine = Engine {
        demo,
        cfg: None,
        bars: vec![],
        screens: json!([]),
        workers: HashMap::new(),
        watcher: None,
        geometry: false,
        pointer: None,
        compositor: json!({}),
        compositor_error: "connecting".into(),
        error: String::new(),
        action_error: String::new(),
        feedback: json!({}),
        serial: String::new(),
        tx,
        alert: Arc::new(Mutex::new(battery::Alert::new())),
        panels: Arc::new(actions::Panels::default()),
        artwork: Arc::new(cache::Artwork::new()),
        history: Arc::new(cache::History::new()),
        lyrics: Arc::new(lyrics::Lyrics::new()),
        tasks: vec![],
    };
    let mut reader = BufReader::new(tokio::io::stdin());
    let mut raw = Vec::new();
    let mut reload = tokio::time::interval(Duration::from_secs(1));
    let mut publish = tokio::time::interval(Duration::from_millis(30));
    let mut dirty = false;
    loop {
        tokio::select! {read=async {ensure!(raw.len()<=1048576,"helper input limit exceeded");Ok::<_,anyhow::Error>((&mut reader).take(1048577-raw.len() as u64).read_until(b'\n',&mut raw).await?)}=>{let n=read?;if n==0{break;}ensure!(raw.len()<=1048576,"helper input limit exceeded");if let Ok(msg)=serde_json::from_slice::<Value>(&raw){match engine.message(msg).await{Ok(changed)=>dirty|=changed,Err(e)=>{engine.error=e.to_string();dirty=true;}}}raw.clear();},Some(event)=rx.recv()=>{match event{Event::Provider(key,state)=>{if let Some(w)=engine.workers.get_mut(&key){w.state=state;dirty=true;}},Event::Compositor(data,error)=>{engine.compositor=data;engine.compositor_error=error;dirty=true;},Event::Emit(v)=>crate::emit(&v),Event::Action(error,feedback)=>{engine.action_error=error;if truth(&feedback){engine.feedback=feedback;}dirty=true;}}},_=reload.tick()=>{match engine.reload(false).await{Ok(changed)=>dirty|=changed,Err(e)=>{let error=e.to_string();dirty|=engine.error!=error;engine.error=error;}}},_=publish.tick()=>{if dirty{engine.publish();dirty=false;}}}
    }
    engine.tasks.clear();
    engine.panels.cancel();
    engine.panels.close(None).await;
    Ok(())
}
