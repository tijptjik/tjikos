//! Directed synthetic showcase. Captures are exclusive-create and outside Git.
use crate::{
    comparison as cmp,
    demo::{self, Client, APP},
    telegram_text::text,
    util::*,
};
use anyhow::{ensure, Context, Result};
use clap::Parser;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn monitors() -> Result<Value> {
    Ok(serde_json::from_slice(&run(
        Command::new("hyprctl").args(["-j", "monitors"]),
        Duration::from_secs(5),
    )?)?)
}
static CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
extern "C" fn cancel(_: i32) {
    CANCEL.store(true, std::sync::atomic::Ordering::Relaxed);
}
fn cancelled() -> bool {
    CANCEL.load(std::sync::atomic::Ordering::Relaxed)
}
struct Output(Option<String>);
impl Drop for Output {
    fn drop(&mut self) {
        if let Some(name) = &self.0 {
            let _ = run(
                Command::new("hyprctl").args(["output", "remove", name]),
                Duration::from_secs(5),
            );
        }
    }
}
struct Session;
impl Drop for Session {
    fn drop(&mut self) {
        let _ = cmp::stop();
    }
}
struct Cycle {
    client: Client,
    speed: f64,
    started: Instant,
    timeline: Vec<Value>,
}
impl Cycle {
    fn pause(&self, s: f64) {
        let until = Instant::now() + Duration::from_secs_f64((s * self.speed).max(0.12));
        while !cancelled() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn control(&mut self, mut v: Value) -> Result<Value> {
        ensure!(!cancelled(), "Demo cancelled");
        v["op"] = json!("cycle");
        cmp::call(self.client.session.as_ref().unwrap(), &v)
    }
    fn point(&mut self, id: &str, target: &str, click: bool) -> Result<Value> {
        self.control(json!({"id":id,"target":target,"click":click}))
    }
    fn wait(&mut self, v: Value, check: impl Fn(&Value) -> bool) -> Result<Value> {
        let start = Instant::now();
        loop {
            if let Ok(r) = self.control(v.clone()) {
                if check(&r) {
                    return Ok(r);
                }
            }
            ensure!(
                !cancelled() && start.elapsed() < Duration::from_secs(8),
                "Demo cancelled or timed out waiting for UI"
            );
            std::thread::sleep(Duration::from_millis(80));
        }
    }
    fn reply(&mut self, id: &str, quote: bool) -> Result<()> {
        self.point(
            id,
            if quote {
                "telegram-quote"
            } else {
                "telegram-compose"
            },
            true,
        )?;
        self.pause(0.5);
        let s = if quote {
            "Our new captain?"
        } else {
            "On my way. Bringing a scanner."
        };
        for end in (3..s.len() + 3).step_by(3) {
            self.control(json!({"id":id,"text":&s[..end.min(s.len())]}))?;
            self.pause(0.12);
        }
        self.pause(0.4);
        self.point(id, "telegram-send", false)?;
        self.pause(0.3);
        self.point(id, "telegram-send", true)?;
        self.pause(0.5);
        Ok(())
    }
    fn selected_robots(&self) -> Result<Vec<PathBuf>> {
        let images = demo::robots(&self.client.root)?;
        [6, 9, 44, 62, 66, 68, 71, 76, 79, 81, 84, 88]
            .iter()
            .map(|n| {
                images
                    .iter()
                    .find(|p| {
                        p.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with(&format!("robot-{n:02}-"))
                    })
                    .cloned()
                    .context("Missing showcase robot")
            })
            .collect()
    }
    fn intro(&mut self, scenes: &[Value]) -> Result<()> {
        let images = self.selected_robots()?;
        let mut cards = vec![];
        for scene in scenes {
            let name = text(&scene["name"]);
            if name == "robots" {
                continue;
            }
            let mut fixtures = demo::fixtures(&self.client.root, name)?;
            if name != "urgency" {
                fixtures.truncate(1)
            }
            for f in fixtures {
                let i = cards.len();
                let mut n = json!({"id":format!("intro-{i}"),"app":APP,"type":"standard","body":"","urgency":"normal","actions":[],"data":{},"icon":""});
                merge(&mut n, &f);
                merge(
                    &mut n,
                    &json!({"image":uri(&images[i%images.len()])?,"received":chrono::Utc::now().timestamp_millis(),"remaining":0,"generation":i,"groupCount":if name=="grouped"{3}else{1},"demoOrder":i,"demoLabel":format!("{}{}",text(&scene["label"]),if name=="urgency"{format!(" · {}",text(&f["urgency"]))}else{String::new()})}),
                );
                cards.push(n);
            }
        }
        self.control(json!({"title":"Notjik","caption":"A robot for every feeling","counter":"","introCards":cards,"intro":true}))?;
        self.pause(2.);
        Ok(())
    }
    fn scene(&mut self, scene: &Value, index: usize, total: usize) -> Result<()> {
        let name = text(&scene["name"]);
        self.client.clear()?;
        self.control(json!({"hidePointer":true,"intro":false,"title":scene["label"],"caption":scene["caption"],"counter":format!("{index:02} / {total:02}")}))?;
        self.pause(if index == 1 { 0.85 } else { 0.55 });
        if index == 1 {
            self.control(json!({"introCards":[]}))?;
        }
        self.client.grouping(name == "grouped")?;
        let start = self.started.elapsed().as_secs_f64();
        if name == "robots" {
            for image in self.selected_robots()? {
                let stem = image.file_stem().unwrap().to_string_lossy();
                self.client.call(&json!({"op":"send","notice":{"app":APP,"id":"demo-robots","group":"robots","title":stem.get(9..).unwrap_or(&stem).replace('-'," "),"body":"Shipboard companion unit","image":uri(&image)?,"data":{"demoRobot":true},"urgency":if stem.contains("-alert-"){"critical"}else if stem.contains("-dream-"){"low"}else{"normal"},"timeout":0,"transient":true}}))?;
                std::thread::sleep(Duration::from_millis(250));
            }
            self.pause(1.);
        } else {
            let mut ids = vec![];
            let mut notices = vec![];
            for (i, f) in demo::fixtures(&self.client.root, name)?
                .into_iter()
                .enumerate()
            {
                let mut n = json!({"app":APP,"id":format!("cycle-{name}-{i}"),"group":if name=="grouped"{name.into()}else{format!("{name}-{i}")},"timeout":0,"transient":true});
                merge(&mut n, &f);
                if name == "timer" {
                    n["data"]["seconds"] = json!(3);
                }
                ids.push(
                    text(&self.client.call(&json!({"op":"send","notice":n}))?["id"]).to_string(),
                );
                notices.push(n);
                if name == "urgency" || name == "grouped" {
                    self.pause(0.55);
                }
            }
            let id = (if name == "grouped" {
                ids.first()
            } else {
                ids.last()
            })
            .context("No scene fixtures")?;
            self.wait(json!({"id":id}), |_| true)?;
            self.pause(1.);
            if ["error", "actions-multiple", "grouped"].contains(&name)
                || name.starts_with("telegram-")
            {
                self.point(id, "card", false)?;
                self.pause(0.55);
            }
            match name {
                "error" => {
                    self.point(id, "cycle-action-0", false)?;
                    self.pause(0.7);
                    self.point(id, "cycle-action-0", true)?;
                    self.pause(0.5);
                }
                "actions-multiple" => {
                    for target in ["cycle-action-1", "cycle-action-0"] {
                        self.point(id, target, false)?;
                        self.pause(0.85);
                    }
                }
                "timer" => self.pause(2.5),
                "update" => {
                    for value in [30, 60, 85, 100] {
                        let mut n = notices[0].clone();
                        n["data"] = json!({"value":value,"label":if value==100{"Sector mapped"}else{"Receiving star charts"}});
                        ensure!(
                            self.client.call(&json!({"op":"send","notice":n}))?["id"] == *id,
                            "Notification update changed identity"
                        );
                        self.pause(0.6);
                    }
                }
                "grouped" => {
                    self.point(id, "group", false)?;
                    // Current group marks are informational; use the public
                    // history command instead of the retired hover shortcut.
                    self.client.call(&json!({"op":"history","mode":"show"}))?;
                    let start = Instant::now();
                    loop {
                        let v = self.client.call(&json!({"op":"status"}))?;
                        if v["drawerOpen"] == true && v["historyCount"].as_u64().unwrap_or(0) >= 3 {
                            break;
                        }
                        ensure!(
                            start.elapsed() < Duration::from_secs(8),
                            "Group history did not open"
                        );
                        std::thread::sleep(Duration::from_millis(80));
                    }
                    self.pause(2.);
                }
                "telegram-text" => self.reply(id, false)?,
                "telegram-photo" => {
                    self.point(id, "telegram-read", false)?;
                    self.pause(0.5);
                    self.point(id, "telegram-read", true)?;
                    self.pause(0.9);
                }
                "telegram-voice" => {
                    self.point(id, "telegram-play", true)?;
                    self.pause(2.);
                    self.point(id, "telegram-play", true)?;
                    self.pause(0.2);
                    self.point(id, "telegram-record", true)?;
                    self.wait(json!({"id":id}), |v| v["recording"] == "recording")?;
                    self.pause(3.2);
                    self.point(id, "telegram-record-send", false)?;
                    self.pause(0.3);
                    self.point(id, "telegram-record-send", true)?;
                    self.pause(0.7);
                }
                "telegram-gif" => self.reply(id, true)?,
                _ => self.pause(1.),
            }
            self.control(json!({"hidePointer":true}))?;
        }
        self.client.clear()?;
        self.control(json!({"closeDrawer":true}))?;
        std::thread::sleep(Duration::from_secs_f64((0.7 * self.speed).max(0.7)));
        self.timeline
            .push(json!({"scene":name,"start":start,"end":self.started.elapsed().as_secs_f64()}));
        println!("{index}/{total} {name}");
        Ok(())
    }
}
pub fn main(root: &Path, args: &[String]) -> Result<()> {
    CANCEL.store(false, std::sync::atomic::Ordering::Relaxed);
    unsafe {
        libc::signal(libc::SIGINT, cancel as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, cancel as *const () as libc::sighandler_t);
    }
    #[derive(Parser)]
    struct Options {
        #[arg(value_parser=["cycle","record"])]
        mode: String,
        output: Option<PathBuf>,
        #[arg(long)]
        monitor: Option<String>,
        #[arg(long,num_args=1..)]
        only: Vec<String>,
        #[arg(long, default_value_t = 1.)]
        speed: f64,
        #[arg(long)]
        plan: bool,
    }
    let o = Options::parse_from(std::iter::once("demo".to_string()).chain(args.iter().cloned()));
    ensure!(
        (0.1..=4.).contains(&o.speed),
        "--speed must be between 0.1 and 4"
    );
    ensure!(
        (o.mode == "record") == o.output.is_some(),
        "record requires OUTPUT.mp4; cycle takes no output path"
    );
    let all: Vec<Value> = serde_json::from_str(include_str!("../examples/demo-scenes.json"))?;
    let scenes = if o.only.is_empty() {
        all.clone()
    } else {
        o.only
            .iter()
            .map(|name| {
                all.iter()
                    .find(|s| s["name"] == *name)
                    .cloned()
                    .context("Unknown scene")
            })
            .collect::<Result<Vec<_>>>()?
    };
    if o.plan {
        for (i, s) in scenes.iter().enumerate() {
            println!("{}. {} — {}", i + 1, text(&s["name"]), text(&s["label"]));
        }
        return Ok(());
    }
    let output = o.output.map(|p| expand(&p.to_string_lossy())).map(|p| {
        if p.is_absolute() {
            p
        } else {
            std::env::current_dir().unwrap().join(p)
        }
    });
    if let Some(p) = &output {
        ensure!(
            p.extension().is_some_and(|s| s.eq_ignore_ascii_case("mp4"))
                && !p.exists()
                && !p.with_extension("chapters.json").exists(),
            "choose a new .mp4 output file; existing files are never overwritten"
        );
        let mut ancestor = p.parent();
        while let Some(dir) = ancestor {
            ensure!(
                !dir.join(".git").exists(),
                "Captures must stay outside a repository"
            );
            ancestor = dir.parent();
        }
    }
    let temp = tempfile::Builder::new().prefix("notjik-cycle-").tempdir()?;
    let mut owned = Output(None);
    let before = monitors()?;
    let monitor = if let Some(name) = &o.monitor {
        ensure!(
            before
                .as_array()
                .context("Invalid monitors")?
                .iter()
                .any(|m| m["name"] == *name),
            "Unknown output"
        );
        name.clone()
    } else if output.is_none() {
        text(
            &before
                .as_array()
                .context("Invalid monitors")?
                .iter()
                .find(|m| m["focused"] == true)
                .unwrap_or(&before[0])["name"],
        )
        .to_string()
    } else {
        run(
            Command::new("hyprctl").args(["output", "create", "headless"]),
            Duration::from_secs(5),
        )?;
        let start = Instant::now();
        let name = loop {
            let current = monitors()?;
            let new = current
                .as_array()
                .context("Invalid monitors")?
                .iter()
                .filter(|m| {
                    !before
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|old| old["name"] == m["name"])
                })
                .collect::<Vec<_>>();
            ensure!(new.len() <= 1, "Ambiguous new recording output");
            if let Some(m) = new.first() {
                break text(&m["name"]).to_string();
            }
            ensure!(
                start.elapsed() < Duration::from_secs(3),
                "Headless recording output did not appear"
            );
            std::thread::sleep(Duration::from_millis(100));
        };
        owned.0 = Some(name.clone());
        let response = run(
            Command::new("hyprctl").arg("eval").arg(format!(
                "hl.monitor({{output={},mode=\"1000x680@60\",position=\"10000x10000\",scale=1}})",
                json!(name)
            )),
            Duration::from_secs(5),
        )?;
        ensure!(
            String::from_utf8_lossy(&response).trim() == "ok",
            "Cannot configure recording output"
        );
        name
    };
    std::env::set_var("NOTJIK_DEMO_CYCLE_DIR", temp.path());
    std::env::set_var("NOTJIK_DEMO_CYCLE_MONITOR", &monitor);
    let session = cmp::start(root, None)?;
    let _session = Session;
    let mut cycle = Cycle {
        client: Client::new(root, Some(session))?,
        speed: o.speed,
        started: Instant::now(),
        timeline: vec![],
    };
    let mut recorder = None;
    let mut partial = None;
    if let Some(p) = &output {
        cycle.control(json!({"title":"Notjik","caption":"A robot for every feeling","counter":"","intro":true}))?;
        std::thread::sleep(Duration::from_millis(2500));
        fs::create_dir_all(p.parent().unwrap())?;
        let pending = tempfile::Builder::new()
            .prefix(".notjik-record-")
            .suffix(".mp4")
            .tempfile_in(p.parent().unwrap())?
            .into_temp_path();
        fs::remove_file(&pending)?; // wf-recorder must create its output without an overwrite prompt.
        let log = fs::File::create(temp.path().join("recorder.log"))?;
        let mut cmd = Command::new("wf-recorder");
        cmd.args([
            "-D",
            "--no-dmabuf",
            "-o",
            &monitor,
            "-r",
            "60",
            "-c",
            "libx264rgb",
            "-p",
            "preset=ultrafast",
            "-p",
            "crf=0",
            "-x",
            "bgr0",
        ]);
        if o.monitor.is_some() {
            let all = monitors()?;
            let m = all
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["name"] == monitor)
                .context("Missing output")?;
            let scale = m["scale"].as_f64().unwrap_or(1.);
            let x = m["x"].as_f64().unwrap_or(0.)
                + (m["width"].as_f64().unwrap_or(1000.) / scale - 1000.) / 2.;
            let y = m["y"].as_f64().unwrap_or(0.)
                + (m["height"].as_f64().unwrap_or(680.) / scale - 680.) / 2.;
            cmd.arg("-g")
                .arg(format!("{},{} 1000x680", x.round(), y.round()));
        }
        cmd.arg("-f")
            .arg(pending.as_os_str())
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        let child = cmd.spawn()?;
        recorder = Some(child);
        partial = Some(pending);
        cycle.started = Instant::now();
    }
    let result = (|| -> Result<()> {
        cycle.intro(&all)?;
        for (i, s) in scenes.iter().enumerate() {
            cycle
                .scene(s, i + 1, scenes.len())
                .with_context(|| format!("Showcase scene {}", text(&s["name"])))?;
        }
        cycle.control(json!({"title":"github.com/tijptjik/tjikos","caption":"Source · Installation · Documentation","counter":"","outro":true}))?;
        cycle.pause(3.);
        Ok(())
    })();
    if let Some(mut child) = recorder {
        unsafe {
            libc::kill(child.id() as i32, libc::SIGINT);
        }
        let start = Instant::now();
        while child.try_wait()?.is_none() && start.elapsed() < Duration::from_secs(15) {
            std::thread::sleep(Duration::from_millis(50));
        }
        if child.try_wait()?.is_none() {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Recorder did not stop");
        }
    }
    result?;
    ensure!(!cancelled(), "Demo cancelled");
    if let (Some(p), Some(partial)) = (output, partial) {
        let partial_path: &Path = partial.as_ref();
        run(
            Command::new("ffprobe")
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "format=duration",
                    "-of",
                    "default=nw=1:nk=1",
                ])
                .arg(partial_path),
            Duration::from_secs(10),
        )?;
        let playable = temp.path().join("playable.mp4");
        run_cancelled(
            Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(partial_path)
                .args([
                    "-c:v",
                    "libx264",
                    "-preset",
                    "slow",
                    "-crf",
                    "14",
                    "-pix_fmt",
                    "yuv420p",
                    "-profile:v",
                    "high",
                    "-movflags",
                    "+faststart",
                ])
                .arg(&playable),
            Duration::from_secs(600),
            Some(&CANCEL),
        )?;
        ensure!(!cancelled(), "Demo cancelled");
        fs::copy(playable, partial_path)?;
        let manifest = p.with_extension("chapters.json");
        let mut chapters = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&manifest)?;
        if let Err(e) = fs::hard_link(partial_path, &p) {
            let _ = fs::remove_file(manifest);
            return Err(e.into());
        }
        use std::io::Write;
        writeln!(
            chapters,
            "{}",
            serde_json::to_string_pretty(&json!({"synthetic":true,"scenes":cycle.timeline}))?
        )?;
        println!("Recording: {}", p.display());
    }
    Ok(())
}
