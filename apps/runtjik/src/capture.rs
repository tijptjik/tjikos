use crate::{
    capture_worker,
    discovery::{folded, Entry},
    paths,
    util::*,
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread::JoinHandle,
    time::Duration,
};
pub fn hypr(kind: &str) -> Result<Vec<Value>> {
    Ok(serde_json::from_str(&command(&["hyprctl", "-j", kind])?)?)
}
pub fn workspace(m: &Value) -> &Value {
    if m["specialWorkspace"]["id"].as_i64().unwrap_or(0) != 0 {
        &m["specialWorkspace"]
    } else {
        &m["activeWorkspace"]
    }
}
pub fn directory(value: &str) -> Result<PathBuf> {
    let path = if let Some(value) = value.strip_prefix("xdg:") {
        let (kind, suffix) = value.split_once('/').unwrap_or((value, ""));
        ensure!(
            ["VIDEOS", "PICTURES"].contains(&kind)
                && !Path::new(suffix)
                    .components()
                    .any(|c| c == std::path::Component::ParentDir),
            "Invalid capture directory"
        );
        let mut base = home().join(if kind == "VIDEOS" {
            "Videos"
        } else {
            "Pictures"
        });
        if let Ok(text) = fs::read_to_string(xdg("config").parent().unwrap().join("user-dirs.dirs"))
        {
            for line in text.lines() {
                if let Some(value) = line.strip_prefix(&format!("XDG_{kind}_DIR=")) {
                    if let Some(words) = shlex::split(value) {
                        if words.len() == 1 {
                            base =
                                PathBuf::from(words[0].replace("$HOME", &home().to_string_lossy()));
                        }
                    }
                }
            }
        }
        base.join(suffix)
    } else if let Some(p) = value.strip_prefix("~/") {
        home().join(p)
    } else {
        PathBuf::from(value)
    };
    ensure!(
        path.is_absolute(),
        "Capture directory must be absolute or start with ~/"
    );
    let mut suffix = Vec::new();
    let mut parent = path.as_path();
    while !parent.exists() {
        suffix.push(
            parent
                .file_name()
                .context("Invalid capture directory")?
                .to_owned(),
        );
        parent = parent.parent().context("Invalid capture directory")?;
    }
    let mut path = fs::canonicalize(parent)?;
    for s in suffix.iter().rev() {
        path.push(s);
    }
    let app = paths::root();
    ensure!(
        !path.starts_with(&app),
        "Captures must be stored outside the application directory"
    );
    for ancestor in app.ancestors() {
        if ancestor.join(".git").exists() {
            ensure!(
                !path.starts_with(ancestor),
                "Captures must be stored outside the repository"
            );
            break;
        }
    }
    Ok(path)
}
#[derive(Default)]
pub struct Captures {
    workers: HashMap<String, JoinHandle<()>>,
    failed: HashSet<String>,
    pub pending: bool,
}
fn row(name: &str, key: &str, title: &str, subtitle: &str, mut data: Value) -> Entry {
    data["keep_open"] = json!(data.get("capture").is_none() && data.get("file").is_none());
    let action = data["capture"].as_str().unwrap_or(
        if ["screen", "workspace", "window", "region", "history", "stop"].contains(&key) {
            key
        } else {
            "history"
        },
    );
    let icon = paths::root().join(format!(
        "qml/icons/capture-{}-{action}.svg",
        if name == "Recorder" { "record" } else { "shot" }
    ));
    Entry {
        id: format!("{name}:{key}"),
        name: title.into(),
        subtitle: subtitle.into(),
        provider: name.into(),
        icon: icon.to_string_lossy().into_owned(),
        keywords: String::new(),
        data,
    }
}
fn message(text: &str) -> Value {
    json!({"kind":"message","message":text})
}
impl Captures {
    fn thumbnail(&mut self, path: &Path, recording: bool) -> Result<String> {
        if !recording {
            return Ok(path.to_string_lossy().into_owned());
        }
        let stamp = path.metadata()?;
        let key = format!(
            "{:x}",
            Sha256::digest(format!(
                "{}:{}:{}",
                path.display(),
                stamp.mtime() as i128 * 1_000_000_000 + stamp.mtime_nsec() as i128,
                stamp.len()
            ))
        );
        let cache = xdg("cache").join("thumbnails");
        private_dir(&cache)?;
        let target = cache.join(format!("{key}.jpg"));
        if target.exists() {
            return Ok(target.to_string_lossy().into_owned());
        }
        if self.failed.contains(&key) {
            return Ok(String::new());
        }
        if self.workers.get(&key).is_some_and(|h| h.is_finished()) {
            let _ = self.workers.remove(&key).unwrap().join();
            if !target.exists() {
                self.failed.insert(key);
            }
            return Ok(String::new());
        }
        if !self.workers.contains_key(&key) && self.workers.len() < 2 {
            let path = path.to_owned();
            self.workers.insert(
                key,
                std::thread::spawn(move || {
                    let temporary = target.with_extension("tmp.jpg");
                    let result = command(&[
                        "ffmpeg",
                        "-v",
                        "error",
                        "-nostdin",
                        "-y",
                        "-i",
                        &path.to_string_lossy(),
                        "-frames:v",
                        "1",
                        "-vf",
                        "scale=480:-1",
                        &temporary.to_string_lossy(),
                    ]);
                    if result.is_ok() {
                        let _ = fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600));
                        let _ = fs::rename(&temporary, target);
                    }
                    let _ = fs::remove_file(temporary);
                }),
            );
        }
        self.pending = true;
        Ok(String::new())
    }
    pub fn query(
        &mut self,
        name: &str,
        query: &str,
        settings: &Value,
        demo: bool,
    ) -> Result<Vec<Entry>> {
        self.pending = false;
        let finished: Vec<_> = self
            .workers
            .iter()
            .filter(|(_, h)| h.is_finished())
            .map(|(k, _)| k.clone())
            .collect();
        for k in finished {
            let _ = self.workers.remove(&k).unwrap().join();
            if !xdg("cache")
                .join("thumbnails")
                .join(format!("{k}.jpg"))
                .exists()
            {
                self.failed.insert(k);
            }
        }
        let rec = name == "Recorder";
        let base = if rec { "/rec" } else { "/grab" };
        let term = query.split_once(' ').map(|(_, t)| t.trim()).unwrap_or("");
        if term == "history" || term.starts_with("history ") {
            return self.history(name, term[7..].trim(), settings, demo);
        }
        if ["screen", "window"].contains(&term) || (rec && term == "workspace") {
            if demo {
                return Ok(vec![row(
                    name,
                    "target",
                    &format!("Demo {term}"),
                    "Synthetic target · no capture",
                    json!({"capture":term,"target":"demo"}),
                )]);
            }
            if term == "window" {
                if rec {
                    return Ok(vec![row(
                        name,
                        "portal",
                        "Choose a window",
                        "Use the desktop sharing picker; recording follows the selected window",
                        json!({"capture":"window"}),
                    )]);
                }
                let mut clients = hypr("clients")?;
                clients.retain(|c| c["mapped"] == true && c["hidden"] != true);
                clients.sort_by_key(|c| {
                    folded(
                        c["title"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .unwrap_or(s(&c["class"])),
                    )
                });
                return Ok(clients
                    .iter()
                    .map(|c| {
                        row(
                            name,
                            s(&c["address"]),
                            c["title"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .unwrap_or(s(&c["class"])),
                            &format!(
                                "Workspace {} · visible window area",
                                s(&c["workspace"]["name"])
                            ),
                            json!({"capture":term,"target":c["address"]}),
                        )
                    })
                    .collect());
            }
            let mut monitors = hypr("monitors")?;
            monitors.sort_by_key(|m| {
                folded(if term == "screen" {
                    s(&m["name"])
                } else {
                    s(&workspace(m)["name"])
                })
            });
            return Ok(monitors
                .iter()
                .map(|m| {
                    row(
                        name,
                        s(&m["name"]),
                        &if term == "screen" {
                            s(&m["name"]).into()
                        } else {
                            format!("Workspace {}", s(&workspace(m)["name"]))
                        },
                        &if term == "screen" {
                            s(&m["description"]).into()
                        } else {
                            format!(
                                "{} · stops when this monitor changes workspace",
                                s(&m["name"])
                            )
                        },
                        json!({"capture":term,"target":m["name"],"workspace":workspace(m)["id"]}),
                    )
                })
                .collect());
        }
        let mut rows = vec![
            row(
                name,
                "region",
                "Region",
                "Drag to select an area · Escape cancels",
                json!({"capture":"region"}),
            ),
            row(
                name,
                "window",
                "Window",
                "Choose a window",
                json!({"edit":format!("{base} window")}),
            ),
        ];
        if rec {
            rows.push(row(
                name,
                "workspace",
                "Workspace",
                "Stops when this monitor changes workspace",
                json!({"edit":format!("{base} workspace")}),
            ));
        }
        rows.push(row(
            name,
            "screen",
            "Screen",
            if rec {
                "Keeps recording across workspace changes"
            } else {
                "Capture a monitor"
            },
            json!({"edit":format!("{base} screen")}),
        ));
        rows.push(row(
            name,
            "history",
            if rec {
                "Previous recordings"
            } else {
                "Previous screenshots"
            },
            "Browse your gallery · Enter opens it",
            json!({"edit":format!("{base} history")}),
        ));
        if rec {
            let status = if demo {
                Some(json!({"file":"Synthetic recording.mp4"}))
            } else {
                capture_worker::request("status")?
            };
            if let Some(status) = status {
                rows.push(row(
                    name,
                    "stop",
                    "Stop recording",
                    Path::new(s(&status["file"]))
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(""),
                    json!({"stop":true}),
                ));
            }
        }
        rows.retain(|r| {
            term.is_empty() || folded(&format!("{} {}", r.name, r.subtitle)).contains(&folded(term))
        });
        Ok(rows)
    }
    fn history(
        &mut self,
        name: &str,
        term: &str,
        settings: &Value,
        demo: bool,
    ) -> Result<Vec<Entry>> {
        let rec = name == "Recorder";
        if demo {
            let cache = xdg("cache").join("demo-thumbnails");
            private_dir(&cache)?;
            let mut rows = vec![];
            for i in 0..9 {
                let path = cache.join(format!("{i}.svg"));
                let color = ["#eb6f92", "#9ccfd8", "#c4a7e7"][i % 3];
                atomic(&path,format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"480\" height=\"270\"><rect width=\"480\" height=\"270\" fill=\"#191724\"/><rect x=\"24\" y=\"24\" width=\"432\" height=\"222\" rx=\"18\" fill=\"{color}\"/><circle cx=\"{}\" cy=\"125\" r=\"45\" fill=\"#26233a\"/><path d=\"M50 220 L210 80 L350 220\" fill=\"#403a52\"/></svg>",90+i*25).as_bytes())?;
                rows.push(row(
                    name,
                    &format!("demo-file-{i}"),
                    &format!("Demo capture {}", i + 1),
                    "Synthetic capture history",
                    json!({"thumbnail":path.to_string_lossy()}),
                ));
            }
            return Ok(rows);
        }
        let options = &settings["plugin_settings"][name];
        let dir = directory(s(&options["directory"]))?;
        let mut files = vec![];
        if dir.exists() {
            for e in fs::read_dir(&dir)? {
                let p = e?.path();
                let ext = p
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if (if rec {
                    ["mp4", "mkv", "webm"].contains(&ext.as_str())
                } else {
                    ["png", "jpg", "jpeg", "webp"].contains(&ext.as_str())
                }) && p.is_file()
                    && !p.is_symlink()
                    && folded(&p.file_name().unwrap().to_string_lossy()).contains(&folded(term))
                {
                    if let Ok(meta) = p.metadata() {
                        files.push((meta.modified()?, p));
                    }
                }
            }
        }
        files.sort_by(|a, b| b.cmp(a));
        let active = if rec {
            capture_worker::request("status")?
        } else {
            None
        };
        let mut rows = vec![];
        for (stamp, p) in files
            .into_iter()
            .take(options["max_results"].as_u64().unwrap_or(40) as usize)
        {
            if active
                .as_ref()
                .is_some_and(|s| s["file"] == p.to_string_lossy().as_ref())
            {
                continue;
            }
            let date: chrono::DateTime<chrono::Local> = stamp.into();
            let thumb = self.thumbnail(&p, rec)?;
            rows.push(row(
                name,
                &p.to_string_lossy(),
                &p.file_name().unwrap().to_string_lossy(),
                &format!("{} · {}", date.format("%Y-%m-%d %H:%M"), dir.display()),
                json!({"file":p.to_string_lossy(),"thumbnail":thumb}),
            ));
        }
        if rows.is_empty() {
            rows.push(row(
                name,
                "empty",
                if !term.is_empty() {
                    "No matching captures"
                } else if rec {
                    "No recordings yet"
                } else {
                    "No screenshots yet"
                },
                if !term.is_empty() {
                    "Try another search or clear the filter."
                } else if rec {
                    "Record your screen, a window, or a region."
                } else {
                    "Capture your screen, a window, or a region."
                },
                json!({"empty_state":true,"disabled":true}),
            ));
        }
        Ok(rows)
    }
    pub fn activate(entry: &Entry, settings: &Value, mode: &str, demo: bool) -> Result<Value> {
        let data = &entry.data;
        let rec = entry.provider == "Recorder";
        let options = &settings["plugin_settings"][&entry.provider];
        if let Some(edit) = data.get("edit") {
            return Ok(json!({"kind":"edit","text":edit}));
        }
        if demo {
            return Ok(message(
                "Synthetic capture action; no files or recording created",
            ));
        }
        if let Some(file) = data["file"].as_str() {
            let path = Path::new(file);
            ensure!(
                path.parent() == Some(directory(s(&options["directory"]))?.as_path())
                    && path.is_file()
                    && !path.is_symlink(),
                "Capture is no longer available"
            );
            if mode == "close" {
                if rec {
                    ensure!(
                        !capture_worker::request("status")?.is_some_and(|s| s["file"] == file),
                        "Stop recording before moving it to Trash"
                    );
                }
                let mut reply = crate::trash::action(Some(path))?;
                reply["refresh_query"] = json!(if rec { "/rec history" } else { "/grab history" });
                return Ok(reply);
            }
            let mut argv = if rec {
                vec!["xdg-open".into()]
            } else {
                strings(&options["viewer"])
            };
            if !rec && Path::new(&argv[0]).file_name().is_some_and(|s| s == "imv") {
                argv = vec![
                    std::env::current_exe()?.to_string_lossy().into_owned(),
                    "image-viewer".into(),
                    json!({"viewer":argv,"paths":[file]}).to_string(),
                ];
            } else {
                argv.push(file.into());
            }
            dispatch(argv, &json!({}))?;
            return Ok(json!({"kind":"activated"}));
        }
        if data["stop"] == true {
            return Ok(message(if capture_worker::request("stop")?.is_some() {
                "Stopping recording; the file will appear in history when finalized"
            } else {
                "No Runtjik recording is active"
            }));
        }
        if data.get("capture").is_none() {
            return Ok(message("Choose a capture type or previous file"));
        }
        capture(data, options, rec)
    }
}
fn countdown(spec: &Value) -> Result<bool> {
    let seconds = spec["countdown_seconds"].as_u64().unwrap_or(0);
    if seconds == 0 {
        return Ok(true);
    }
    let mut output = s(&spec["output"]).to_owned();
    let monitors = hypr("monitors")?;
    if let Some(area) = spec["area"].as_str() {
        let nums: Vec<i64> = area
            .split([',', ' ', 'x'])
            .filter_map(|n| n.parse().ok())
            .collect();
        if nums.len() == 4 {
            let (x, y) = (nums[0] + nums[2] / 2, nums[1] + nums[3] / 2);
            if let Some(m) = monitors.iter().find(|m| {
                let mx = m["x"].as_i64().unwrap_or(0);
                let my = m["y"].as_i64().unwrap_or(0);
                let scale = m["scale"].as_f64().unwrap_or(1.);
                let (w, h) = if m["transform"].as_i64().unwrap_or(0) % 2 == 0 {
                    (&m["width"], &m["height"])
                } else {
                    (&m["height"], &m["width"])
                };
                x >= mx
                    && y >= my
                    && ((x - mx) as f64) < w.as_f64().unwrap_or(0.) / scale
                    && ((y - my) as f64) < h.as_f64().unwrap_or(0.) / scale
            }) {
                output = s(&m["name"]).into();
            }
        }
    }
    if output.is_empty() {
        output = monitors
            .iter()
            .find(|m| m["focused"] == true)
            .map(|m| s(&m["name"]).to_owned())
            .unwrap_or_default();
    }
    let temp = tempfile::Builder::new()
        .prefix("runtjik-countdown-")
        .tempdir()?;
    let success = temp.path().join("complete");
    let mut child = Command::new("quickshell")
        .args([
            "--path",
            &paths::root().join("qml/countdown").to_string_lossy(),
        ])
        .env("RUNTJIK_COUNTDOWN_SECONDS", seconds.to_string())
        .env("RUNTJIK_COUNTDOWN_OUTPUT", output)
        .env("RUNTJIK_COUNTDOWN_SUCCESS", &success)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(seconds + 15);
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "Could not display recording countdown");
            break;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Recording countdown timed out");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if success.exists() {
        std::thread::sleep(Duration::from_millis(300));
        Ok(true)
    } else {
        Ok(false)
    }
}
fn capture(data: &Value, options: &Value, rec: bool) -> Result<Value> {
    let mode = s(&data["capture"]);
    let tool = if rec && mode == "window" {
        "gpu-screen-recorder"
    } else if rec {
        "wf-recorder"
    } else {
        "grim"
    };
    for tool in std::iter::once(tool)
        .chain((mode == "region").then_some("slurp"))
        .chain((rec && mode != "window" && options["audio"] == true).then_some("pactl"))
    {
        ensure!(which(tool).is_some(), "Install {tool} to use this action");
    }
    if rec {
        ensure!(
            capture_worker::request("status")?.is_none(),
            "A Runtjik recording is already active; stop it first"
        );
    }
    let mut spec = options.clone();
    spec["mode"] = json!(mode);
    spec["area"] = Value::Null;
    spec["output"] = Value::Null;
    spec["workspace"] = Value::Null;
    match mode {
        "region" => {
            let area = match run(&["slurp".into()], None, 120) {
                Ok(s) => s.trim().to_owned(),
                Err(e) => {
                    if e.to_string().trim() == "slurp:"
                        || e.to_string().contains("selection cancelled")
                    {
                        return Ok(message("Capture cancelled"));
                    }
                    return Err(e);
                }
            };
            ensure!(
                regex::Regex::new(r"^-?\d+,-?\d+ [1-9]\d*x[1-9]\d*$")?.is_match(&area),
                "Invalid selected region"
            );
            spec["area"] = json!(area);
        }
        "window" if !rec => {
            let clients = hypr("clients")?;
            let client = clients
                .iter()
                .find(|c| {
                    c["address"] == data["target"] && c["mapped"] == true && c["hidden"] != true
                })
                .context("Window has closed")?;
            crate::windows::focus(client)?;
            std::thread::sleep(Duration::from_millis(300));
            let clients = hypr("clients")?;
            let c = clients
                .iter()
                .find(|c| c["address"] == data["target"])
                .context("Window has closed")?;
            let n = |v: &Value| v.as_i64().context("Invalid window geometry");
            let (w, h) = (n(&c["size"][0])?, n(&c["size"][1])?);
            ensure!(
                w >= 2 && h >= 2,
                "Target must be at least 2 pixels wide and high"
            );
            spec["area"] = json!(format!("{},{} {w}x{h}", n(&c["at"][0])?, n(&c["at"][1])?));
        }
        "screen" | "workspace" => {
            let ms = hypr("monitors")?;
            let m = ms
                .iter()
                .find(|m| m["name"] == data["target"])
                .context("Monitor disconnected")?;
            if mode == "workspace" {
                ensure!(
                    workspace(m)["id"] == data["workspace"],
                    "Workspace changed; choose the target again"
                );
                spec["workspace"] = data["workspace"].clone();
            }
            spec["output"] = m["name"].clone();
        }
        "window" => (),
        _ => bail!("Unknown capture type"),
    }
    if rec && !countdown(&spec)? {
        return Ok(message("Recording cancelled"));
    }
    let directory = directory(s(&options["directory"]))?;
    private_dir(&directory)?;
    let file = directory.join(format!(
        "{}-{}-{}.{}",
        if rec { "Recording" } else { "Screenshot" },
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..8],
        if rec { "mp4" } else { "png" }
    ));
    spec["file"] = json!(file.to_string_lossy());
    if rec {
        let mut child = Command::new(std::env::current_exe()?)
            .args(["capture-worker", &spec.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
            let _ = tx.send(result);
        });
        let reply = rx
            .recv_timeout(Duration::from_secs(5))
            .ok()
            .and_then(Result::ok)
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or(json!({}));
        if reply["ready"] != true {
            unsafe {
                libc::kill(child.id() as i32, libc::SIGTERM);
            }
            let deadline = std::time::Instant::now() + Duration::from_secs(25);
            while child.try_wait()?.is_none() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
            child.wait()?;
            bail!(
                "{}",
                reply["error"]
                    .as_str()
                    .unwrap_or("Recorder did not start; check the Runtjik capture log")
            );
        }
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    } else {
        let mut args = vec!["grim".into()];
        for (k, flag) in [("output", "-o"), ("area", "-g")] {
            if let Some(v) = spec[k].as_str() {
                args.extend([flag.into(), v.into()]);
            }
        }
        args.push(file.to_string_lossy().into_owned());
        run(&args, None, 15)?;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600))?;
        if options["copy_to_clipboard"] == true {
            if let Err(e) = binary(
                &["wl-copy".into(), "--type".into(), "image/png".into()],
                fs::read(&file)?,
                5,
                true,
            ) {
                return Ok(message(&format!(
                    "Saved {}; clipboard copy failed: {e}",
                    file.display()
                )));
            }
        }
    }
    Ok(
        json!({"kind":"activated","close_launcher":true,"message":format!("{}: {}",if rec{"Recording started"}else{"Saved screenshot"},file.display())}),
    )
}
