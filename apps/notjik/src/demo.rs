use crate::{comparison as cmp, telegram_text::text, util::*};
use anyhow::{ensure, Context, Result};
use clap::Parser;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
pub const APP: &str = "notjik demo";
pub fn fixtures(root: &Path, name: &str) -> Result<Vec<Value>> {
    let data: Value = serde_json::from_str(include_str!("../examples/demo-fixtures.json"))?;
    let raw = serde_json::to_string(data[name].get("notices").context("Unknown demo type")?)?;
    let prefix = uri(root)?;
    Ok(serde_json::from_str(
        &raw.replace("file:///NOTJIK_ROOT", &prefix),
    )?)
}
pub fn names() -> Vec<String> {
    serde_json::from_str(include_str!("../examples/demo-order.json")).unwrap()
}
pub fn robots(root: &Path) -> Result<Vec<PathBuf>> {
    let mut result = vec![];
    let re = regex::Regex::new(r"^robot-[0-9][0-9]-.*\.png$")?;
    for dir in [root.join("qml/icons"), root.join("qml/icons/robots")] {
        let mut group = fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| re.is_match(s))
            })
            .collect::<Vec<_>>();
        group.sort();
        result.extend(group);
    }
    Ok(result)
}
pub fn prepare(app: &Path) -> Result<()> {
    let patches: Value = serde_json::from_str(include_str!("../examples/demo-patches.json"))?;
    for p in patches.as_array().unwrap() {
        let path = app.join(text(&p["file"]));
        let source = fs::read_to_string(&path)?;
        let old = text(&p["old"]);
        ensure!(
            source.contains(old),
            "Demo instrumentation needs updating: {}",
            path.display()
        );
        fs::write(path, source.replacen(old, text(&p["new"]), 1))?;
    }
    fs::write(
        app.join("qml/DemoStage.qml"),
        include_str!("../examples/DemoStage.qml"),
    )?;
    Ok(())
}
pub struct Client {
    pub root: PathBuf,
    pub session: Option<Value>,
    ids: Value,
}
impl Client {
    pub fn new(root: &Path, session: Option<Value>) -> Result<Self> {
        let mut ids = json!({});
        if let Some(v) = &session {
            if let Ok(saved) = read_json(&cmp::directory().join("demo-ids.json")) {
                if saved["birth"] == v["birth"] && saved["ids"].is_object() {
                    ids = saved["ids"].clone();
                }
            }
            let start = Instant::now();
            loop {
                if cmp::call(v, &json!({"op":"status"})).is_ok_and(|s| s["lockState"] != "unknown")
                {
                    break;
                }
                ensure!(
                    start.elapsed() < Duration::from_secs(5),
                    "Comparison notjik did not become ready"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        Ok(Self {
            root: root.into(),
            session,
            ids,
        })
    }
    fn save(&self) -> Result<()> {
        if let Some(v) = &self.session {
            private_write(
                &cmp::directory().join("demo-ids.json"),
                json!({"birth":v["birth"],"ids":self.ids})
                    .to_string()
                    .as_bytes(),
            )?;
        }
        Ok(())
    }
    pub fn call(&mut self, r: &Value) -> Result<Value> {
        let result = if let Some(s) = &self.session {
            cmp::call(s, r)?
        } else {
            ipc(&self.root, r)?
        };
        if let Some(s) = &self.session {
            if r["op"] == "send" {
                let id = text(&result["id"]);
                let mut reference = r["notice"].clone();
                reference["image"] =
                    cmp::call(s, &json!({"op":"reference-artwork","id":id}))?["image"].clone();
                let old = self.ids[id].as_u64().unwrap_or(0);
                self.ids[id] = json!(cmp::demo_notify(s, &reference, old)?);
                self.save()?;
            } else if r["op"] == "dismiss" {
                if let Some(id) = self.ids.as_object_mut().unwrap().remove(text(&r["id"])) {
                    cmp::demo_close(s, id.as_u64().unwrap_or(0))?;
                    self.save()?;
                }
            }
        }
        Ok(result)
    }
    pub fn clear(&mut self) -> Result<usize> {
        let v = self.call(&json!({"op":"list"}))?;
        let mut count = 0;
        for n in v["notices"].as_array().into_iter().flatten() {
            if n["app"] == APP {
                match self.call(&json!({"op":"dismiss","id":n["id"]})) {
                    Ok(_) => count += 1,
                    Err(e) if e.to_string().contains("Notification not found") => {}
                    Err(e) => return Err(e),
                }
            }
        }
        if let Some(s) = &self.session {
            let ids = std::mem::replace(&mut self.ids, json!({}));
            for id in ids.as_object().unwrap().values() {
                cmp::demo_close(s, id.as_u64().unwrap_or(0))?;
            }
            self.save()?;
        }
        Ok(count)
    }
    pub fn grouping(&self, enabled: bool) -> Result<()> {
        if let Some(s) = &self.session {
            cmp::grouping(s, enabled)?;
        }
        Ok(())
    }
    pub fn show(&mut self, name: &str, timeout: Option<f64>, log: bool) -> Result<Vec<String>> {
        let mut ids = vec![];
        for (i, f) in fixtures(&self.root, name)?.into_iter().enumerate() {
            let mut n = json!({"app":APP,"id":format!("demo-{name}-{i}"),"group":if ["grouped","telegram-grouped"].contains(&name){name.into()}else{format!("{name}-{i}")},"timeout":0,"transient":!log});
            merge(&mut n, &f);
            if let Some(t) = timeout {
                n["timeout"] = json!(t);
            }
            let id = text(&self.call(&json!({"op":"send","notice":n}))?["id"]).to_string();
            ids.push(id.clone());
            if name == "update" {
                for value in [25, 50, 75, 100] {
                    std::thread::sleep(Duration::from_millis(350));
                    n["data"] = json!({"value":value,"label":"Saving your encrypted archive"});
                    ensure!(
                        self.call(&json!({"op":"send","notice":n}))?["id"] == id,
                        "Notification update changed identity"
                    );
                }
            }
        }
        Ok(ids)
    }
}
pub fn main(root: &Path, args: &[String]) -> Result<()> {
    if args.first().is_some_and(|s| s == "cycle" || s == "record") {
        return crate::cycle::main(root, args);
    }
    #[derive(Parser)]
    #[command(name = "notjikctl demo")]
    struct Options {
        types: Vec<String>,
        #[arg(long)]
        solo: bool,
        #[arg(long)]
        append: bool,
        #[arg(long)]
        interval: Option<String>,
        #[arg(long)]
        timeout: Option<String>,
        #[arg(long)]
        log: bool,
    }
    let o = Options::parse_from(std::iter::once("demo".to_string()).chain(args.iter().cloned()));
    let mut selected = if o.types.is_empty() {
        vec!["overview".into()]
    } else {
        o.types
    };
    let controls = [
        "list", "types", "overview", "all", "robots", "next", "clear",
    ];
    let names = names();
    ensure!(
        selected
            .iter()
            .all(|s| controls.contains(&s.as_str()) || names.contains(s)),
        "Unknown demo type. Run notjikctl demo list."
    );
    ensure!(
        selected.len() == 1 || !selected.iter().any(|s| controls.contains(&s.as_str())),
        "Use demo controls on their own"
    );
    let interval = if let Some(t) = o.interval {
        crate::duration(&t)?
    } else if selected[0] == "robots" {
        1000.
    } else {
        4000.
    };
    ensure!(interval > 0., "--interval must be positive");
    ensure!(
        !o.append || !["all", "robots"].contains(&selected[0].as_str()),
        "all and robots cycle one preview at a time"
    );
    let timeout = o.timeout.as_deref().map(crate::duration).transpose()?;
    if ["list", "types"].contains(&selected[0].as_str()) {
        let f: Value = serde_json::from_str(include_str!("../examples/demo-fixtures.json"))?;
        println!("robots               Quickly preview every installed robot image");
        for n in names {
            println!("{n:20} {}", text(&f[&n]["description"]));
        }
        return Ok(());
    }
    let session = if o.solo {
        None
    } else {
        let _lock = cmp::control_lock()?;
        if selected[0] == "clear" {
            cmp::state()
        } else if cmp::reference(None).is_err() {
            eprintln!("Dunst reference unavailable; showing notjik alone. Install the reference or use compare start --dunst PATH.");
            None
        } else {
            Some(cmp::start(root, None)?)
        }
    };
    let mut client = Client::new(root, session)?;
    if let Some(v) = &client.session {
        println!(
            "Dunst: left · notjik: right (+{}px); isolated comparison sessions",
            v["offsetX"]
        );
    }
    if selected[0] == "clear" {
        println!("Cleared {} demo cards", client.clear()?);
        return Ok(());
    }
    let status = client.call(&json!({"op":"status"}))?;
    ensure!(
        status["paused"] == false,
        "Notifications are paused; run notjikctl resume before previewing demos."
    );
    ensure!(
        status["lockState"] == "unlocked",
        "Unlock the desktop before previewing demos."
    );
    let cursor = if client.session.is_some() {
        cmp::directory()
    } else {
        state()
    }
    .join("demo-cursor.json");
    if selected[0] == "next" {
        let mut previous = read_json(&cursor)
            .ok()
            .and_then(|v| v["last"].as_str().map(str::to_string));
        if !previous.as_ref().is_some_and(|s| names.contains(s)) {
            let list = client.call(&json!({"op":"list"}))?;
            for n in list["notices"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|n| n["app"] == APP)
            {
                let name = text(&n["id"])
                    .rsplit("/demo-")
                    .next()
                    .unwrap_or("")
                    .rsplit_once('-')
                    .map(|s| s.0)
                    .unwrap_or("");
                if names.iter().any(|s| s == name) {
                    previous = Some(name.into());
                }
            }
        }
        selected = vec![names[previous
            .and_then(|s| names.iter().position(|n| n == &s))
            .map(|i| (i + 1) % names.len())
            .unwrap_or(0)]
        .clone()];
    }
    if status["drawerOpen"] == true {
        client.call(&json!({"op":"history","mode":"hide"}))?;
    }
    if !o.append {
        client.clear()?;
    }
    if selected[0] == "robots" {
        client.grouping(false)?;
        let images = robots(root)?;
        ensure!(!images.is_empty(), "No robot artwork is installed");
        for (i, p) in images.iter().enumerate() {
            let stem = p.file_stem().unwrap().to_string_lossy();
            let label = stem
                .strip_prefix("robot-")
                .unwrap_or(&stem)
                .replace('-', ' '.to_string().as_str());
            client.call(&json!({"op":"send","notice":{"app":APP,"id":"demo-robots","group":"robots","title":label,"body":"","image":uri(p)?,"urgency":if stem.contains("-alert-"){"critical"}else if stem.contains("-dream-"){"low"}else{"normal"},"data":{"demoRobot":true},"timeout":timeout.unwrap_or(0.),"transient":!o.log}}))?;
            println!("{label}");
            if i + 1 < images.len() {
                std::thread::sleep(Duration::from_secs_f64(interval / 1000.));
            }
        }
        return Ok(());
    }
    let cycle = selected[0] == "all";
    if cycle {
        selected = names;
    } else if selected[0] == "overview" {
        selected = ["icon", "progress", "error", "timer"]
            .map(str::to_string)
            .to_vec();
    }
    for (i, name) in selected.iter().enumerate() {
        if cycle && i > 0 {
            client.clear()?;
        }
        client.grouping(
            ["grouped", "telegram-grouped"].contains(&name.as_str())
                || (!cycle
                    && selected
                        .iter()
                        .any(|s| ["grouped", "telegram-grouped"].contains(&s.as_str())))
                || (o.append && status["groupingEnabled"] == true),
        )?;
        let ids = client.show(name, timeout, o.log)?;
        private_write(&cursor, json!({"last":name}).to_string().as_bytes())?;
        println!("{}/{} {name}: {}", i + 1, selected.len(), ids.join(", "));
        if client.session.is_some() {
            println!("{}", reference_note(name));
        }
        if cycle && i + 1 < selected.len() {
            std::thread::sleep(Duration::from_secs_f64(interval / 1000.));
        }
    }
    Ok(())
}

fn reference_note(name: &str) -> &str {
    match name {
        "telegram-grouped" => {
            "Dunst: separate text baselines; notjik groups three Telegram messages"
        }
        "status" | "success" | "warning" | "error" => {
            "Dunst: status as text; inline state layout is notjik-only"
        }
        "timer" | "timer-done" => "Dunst: static time as text; live countdown is notjik-only",
        "grouped" => "Dunst: separate notices; grouped history/count is notjik-only",
        "resident" => "Dunst: native actions via middle click/context menu; footer is notjik-only",
        s if s.starts_with("telegram-") => {
            "Dunst: text baseline; media player and Telegram controls are notjik-only"
        }
        s if s.starts_with("actions") => {
            "Dunst: native actions via middle click/context menu; footer is notjik-only"
        }
        _ => "native reference",
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn instrumentation_matches_current_qml() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let temp = tempfile::tempdir().unwrap();
        let patches: Value =
            serde_json::from_str(include_str!("../examples/demo-patches.json")).unwrap();
        for p in patches.as_array().unwrap() {
            let target = temp.path().join(text(&p["file"]));
            if !target.exists() {
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::copy(root.join(text(&p["file"])), target).unwrap();
            }
        }
        prepare(temp.path()).unwrap();
        let recorder = fs::read_to_string(temp.path().join("qml/TelegramRecorder.qml")).unwrap();
        assert!(recorder.contains("Only synthetic demo recordings are allowed"));
        assert!(recorder.contains("cycleSend.restart(); return;"));
    }
}
