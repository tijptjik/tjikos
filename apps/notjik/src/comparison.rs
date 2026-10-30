//! Optional reference UI on private buses. Never changes the desktop bus owner.
use crate::{telegram_text::text, util::*};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader},
    os::{
        fd::AsRawFd,
        unix::{fs::OpenOptionsExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stopped(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}
pub fn directory() -> PathBuf {
    std::env::var_os("NOTJIK_DEMO_CYCLE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".cache/notjik/runtime"))
                .join(format!("notjik-compare-{}", unsafe { libc::getuid() }))
        })
}
fn birth(pid: u32) -> Option<String> {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()?
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)
        .map(str::to_string)
}
pub fn state() -> Option<Value> {
    let v = read_json(&directory().join("session.json")).ok()?;
    let pid = v["pid"].as_u64()? as u32;
    if birth(pid).as_deref() == v["birth"].as_str() {
        Some(v)
    } else {
        None
    }
}
pub fn control_lock() -> Result<fs::File> {
    private_dir(&directory())?;
    let f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(directory().join("control.lock"))?;
    ensure!(
        unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } == 0,
        "Cannot lock comparison controls"
    );
    Ok(f)
}
fn signature(root: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(crate::config::load(root)?.to_string());
    fn walk(p: &Path, hash: &mut Sha256) -> Result<()> {
        let mut entries = fs::read_dir(p)?.collect::<std::result::Result<Vec<_>, _>>()?;
        entries.sort_by_key(|e| e.path());
        for e in entries {
            let p = e.path();
            if p.is_dir() {
                walk(&p, hash)?;
            } else if p.is_file() {
                hash.update(p.file_name().unwrap().as_encoded_bytes());
                hash.update(fs::read(p)?);
            }
        }
        Ok(())
    }
    for dir in ["qml", "themes"] {
        walk(&root.join(dir), &mut hash)?;
    }
    hash.update(fs::read(std::env::current_exe()?)?);
    Ok(format!("{:x}", hash.finalize()))
}
pub fn reference(executable: Option<&str>) -> Result<PathBuf> {
    let p = if let Some(s) = executable {
        expand(s)
    } else {
        let found = std::env::var_os("PATH")
            .into_iter()
            .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .map(|p| p.join("dunst"))
            .find(|p| p.is_file());
        found.unwrap_or_else(|| {
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".cache"))
                .join("notjik/comparison-reference/usr/bin/dunst")
        })
    };
    ensure!(p.is_file(),"Dunst comparison binary missing; pass --dunst /path/to/dunst (production does not require it)");
    Ok(p.canonicalize()?)
}
pub fn start(root: &Path, executable: Option<&str>) -> Result<Value> {
    let signature = signature(root)?;
    if let Some(v) = state() {
        if v["signature"] == signature {
            return Ok(v);
        }
        stop()?;
    }
    let dunst = reference(executable)?;
    private_dir(&directory())?;
    let _ = fs::remove_file(directory().join("session.json"));
    let log = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(directory().join("session.log"))?;
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("compare-serve")
        .arg(root)
        .arg(dunst)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Some(v) = state() {
            return Ok(v);
        }
        ensure!(
            child.try_wait()?.is_none(),
            "Comparison failed to start; see {}",
            directory().join("session.log").display()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    anyhow::bail!(
        "Comparison startup timed out; see {}",
        directory().join("session.log").display()
    )
}
pub fn stop() -> Result<()> {
    if let Some(v) = state() {
        let pid = v["pid"].as_u64().unwrap() as i32;
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
        ensure!(fd >= 0, "Cannot open comparison process");
        let result = (|| -> Result<()> {
            ensure!(
                birth(pid as u32).as_deref() == v["birth"].as_str(),
                "Comparison process changed"
            );
            ensure!(
                unsafe {
                    libc::syscall(
                        libc::SYS_pidfd_send_signal,
                        fd,
                        libc::SIGTERM,
                        std::ptr::null::<libc::siginfo_t>(),
                        0,
                    )
                } == 0,
                "Cannot stop comparison process"
            );
            let start = Instant::now();
            while state().is_some() {
                ensure!(
                    start.elapsed() < Duration::from_secs(10),
                    "Comparison did not stop"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(())
        })();
        unsafe {
            libc::close(fd);
        }
        result?;
    }
    Ok(())
}
fn environment(cmd: &mut Command, v: &Value, name: &str) {
    cmd.env("DBUS_SESSION_BUS_ADDRESS", text(&v["buses"][name]))
        .env_remove("DBUS_STARTER_ADDRESS")
        .env_remove("DBUS_STARTER_BUS_TYPE");
    if name == "notjik" {
        cmd.env("NOTJIK_HOME", text(&v["app"]))
            .env("NOTJIK_CONFIG", directory().join("config.json"))
            .env("NOTJIK_STATE_DIR", directory().join("history"));
    }
}
pub fn call(v: &Value, r: &Value) -> Result<Value> {
    let bytes = serde_json::to_vec(r)?;
    ensure!(bytes.len() <= 131072, "Request exceeds 128 KiB");
    let dir = tempfile::Builder::new()
        .prefix("ipc-")
        .tempdir_in(directory())?;
    let p = dir.path().join("request.json");
    private_write(&p, &bytes)?;
    let mut cmd = Command::new("quickshell");
    cmd.args(["ipc", "--path"])
        .arg(Path::new(text(&v["app"])).join("qml"))
        .args(["call", "notjik", "requestFile"])
        .arg(p);
    environment(&mut cmd, v, "notjik");
    let response: Value = serde_json::from_slice(&run(&mut cmd, Duration::from_secs(5))?)?;
    ensure!(response["ok"] == true, "{}", text(&response["error"]));
    Ok(response)
}
pub fn grouping(v: &Value, enabled: bool) -> Result<()> {
    let p = directory().join("config.json");
    let mut c = read_json(&p)?;
    if c["grouping"]["enabled"] != enabled {
        c["grouping"]["enabled"] = json!(enabled);
        private_write(&p, c.to_string().as_bytes())?;
    }
    let start = Instant::now();
    while call(v, &json!({"op":"status"}))?["groupingEnabled"] != enabled {
        ensure!(
            start.elapsed() < Duration::from_secs(5),
            "Comparison grouping configuration did not reload"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}
pub fn demo_close(v: &Value, id: u64) -> Result<()> {
    run(
        Command::new("busctl")
            .arg(format!("--address={}", text(&v["buses"]["dunst"])))
            .args([
                "call",
                "org.freedesktop.Notifications",
                "/org/freedesktop/Notifications",
                "org.freedesktop.Notifications",
                "CloseNotification",
                "u",
                &id.to_string(),
            ]),
        Duration::from_secs(10),
    )?;
    Ok(())
}
pub fn demo_notify(v: &Value, n: &Value, replaces: u64) -> Result<u64> {
    let d = &n["data"];
    let kind = text(&n["type"]);
    let extra = match kind {
        "status" => [text(&d["state"]), text(&d["detail"])]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · "),
        "timer" => {
            let s = d["seconds"].as_u64().unwrap_or(0);
            format!(
                "{:02}:{:02} · {}",
                s / 60,
                s % 60,
                d[if s > 0 { "label" } else { "doneLabel" }]
                    .as_str()
                    .unwrap_or(if s > 0 { "Remaining" } else { "Ready" })
            )
        }
        "progress" => text(&d["label"]).into(),
        "telegram" => match text(&d["kind"]) {
            "photo" => "Photo preview",
            "voice" => "Voice message · 0:08",
            "sticker" => "Sticker",
            "animation" => "Animation",
            "emoji" => "Animated emoji",
            _ => "Telegram message",
        }
        .into(),
        _ => String::new(),
    };
    let mut body = text(&n["body"]).to_string();
    if !extra.is_empty() {
        if !body.is_empty() {
            body.push('\n')
        }
        body.push_str(&extra);
    }
    let mut args = vec![
        format!("--address={}", text(&v["buses"]["dunst"])),
        "call".into(),
        "org.freedesktop.Notifications".into(),
        "/org/freedesktop/Notifications".into(),
        "org.freedesktop.Notifications".into(),
        "Notify".into(),
        "susssasa{sv}i".into(),
        text(&n["app"]).into(),
        replaces.to_string(),
        text(&n["icon"]).into(),
        text(&n["title"]).into(),
        body,
    ];
    let actions = n["actions"].as_array().cloned().unwrap_or_default();
    args.push((actions.len() * 2).to_string());
    for (i, a) in actions.iter().enumerate() {
        args.push(if i == 0 {
            "default".into()
        } else {
            format!("action-{i}")
        });
        args.push(text(&a["label"]).into());
    }
    let mut hints = vec![
        vec![
            "urgency".into(),
            "y".into(),
            match text(&n["urgency"]) {
                "low" => "0",
                "critical" => "2",
                _ => "1",
            }
            .into(),
        ],
        vec![
            "transient".into(),
            "b".into(),
            n["transient"].as_bool().unwrap_or(true).to_string(),
        ],
        vec![
            "resident".into(),
            "b".into(),
            n["resident"].as_bool().unwrap_or(false).to_string(),
        ],
    ];
    if kind == "progress" {
        hints.push(vec![
            "value".into(),
            "i".into(),
            d["value"].as_f64().unwrap_or(0.).round().to_string(),
        ]);
    }
    if !text(&n["image"]).is_empty() {
        hints.push(vec![
            "image-path".into(),
            "s".into(),
            text(&n["image"]).into(),
        ]);
    }
    args.push(hints.len().to_string());
    args.extend(hints.into_iter().flatten());
    args.push(n["timeout"].as_f64().unwrap_or(0.).round().to_string());
    let out = run(Command::new("busctl").args(args), Duration::from_secs(10))?;
    Ok(String::from_utf8(out)?
        .split_whitespace()
        .nth(1)
        .context("Missing Dunst ID")?
        .parse()?)
}
struct Children(Vec<Child>);
impl Drop for Children {
    fn drop(&mut self) {
        for c in self.0.iter_mut().rev() {
            if c.try_wait().ok().flatten().is_none() {
                unsafe {
                    libc::kill(c.id() as i32, libc::SIGTERM);
                }
                let start = Instant::now();
                while c.try_wait().ok().flatten().is_none()
                    && start.elapsed() < Duration::from_secs(3)
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                let _ = c.kill();
                let _ = c.wait();
            }
        }
        let _ = fs::remove_file(directory().join("session.json"));
    }
}
pub fn serve(root: &Path, dunst: &Path) -> Result<()> {
    unsafe {
        libc::signal(libc::SIGTERM, stopped as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, stopped as *const () as libc::sighandler_t);
    }
    let path = directory();
    private_dir(&path)?;
    let app = path.join("app");
    private_dir(&app)?;
    fn copy_ui(source: &Path, dest: &Path) -> Result<()> {
        fs::create_dir_all(dest)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let p = entry.path();
            let target = dest.join(entry.file_name());
            if p.is_dir() {
                copy_ui(&p, &target)?;
            } else if p
                .extension()
                .is_some_and(|e| e == "qml" || e == "js" || e == "json")
            {
                fs::copy(&p, &target)?;
            } else if !target.exists() {
                std::os::unix::fs::symlink(p.canonicalize()?, target)?;
            }
        }
        Ok(())
    }
    for d in ["qml", "themes"] {
        copy_ui(&root.join(d), &app.join(d))?;
    }
    private_dir(&app.join("bin"))?;
    fs::copy(std::env::current_exe()?, app.join("bin/notjik-core"))?;
    fs::copy(std::env::current_exe()?, app.join("bin/notjikctl"))?;
    let shell = app.join("qml/shell.qml");
    let source = fs::read_to_string(&shell)?;
    let reference = include_str!("../examples/reference-artwork.js");
    ensure!(
        source.contains("case \"status\": break;"),
        "Comparison instrumentation needs updating"
    );
    fs::write(
        &shell,
        source.replacen("case \"status\": break;", reference, 1),
    )?;
    let cycle = std::env::var_os("NOTJIK_DEMO_CYCLE_DIR").is_some();
    if cycle {
        crate::demo::prepare(&app)?;
        fs::write(
            app.join(".synthetic-only"),
            b"No Telegram or microphone access\n",
        )?;
    }
    let mut cfg = crate::config::load(root)?;
    let monitor = std::env::var("NOTJIK_DEMO_CYCLE_MONITOR")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            cfg["placement"]["primaryMonitor"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            run(
                Command::new("hyprctl").args(["-j", "monitors"]),
                Duration::from_secs(5),
            )
            .ok()
            .and_then(|r| serde_json::from_slice::<Value>(&r).ok())
            .and_then(|v| v[0]["name"].as_str().map(str::to_string))
            .unwrap_or_default()
        });
    let width = cfg["style"]["width"].as_f64().unwrap_or(420.);
    merge(
        &mut cfg["placement"],
        &json!({"monitors":"primary","primaryMonitor":monitor,"offsetX":width+40.,"perMonitor":{}}),
    );
    cfg["placement"]["perMonitor"] = json!({});
    cfg["grouping"]["enabled"] = json!(false);
    cfg["history"]["enabled"] = json!(true);
    cfg["history"]["logTransient"] = json!(false);
    for v in cfg["components"].as_object_mut().unwrap().values_mut() {
        *v = json!(url::Url::parse(text(v))?
            .to_file_path()
            .map_err(|_| anyhow::anyhow!("Invalid component"))?);
    }
    let (mut origin, mut offset) = ("bottom-center", "(0, 40)".to_string());
    if cycle {
        cfg["style"]["width"] = json!(420);
        merge(
            &mut cfg["placement"],
            &json!({"offsetX":230,"offsetY":-140,"margin":0}),
        );
        cfg["stack"]["heightPercent"] = json!(65);
        cfg["history"]["logTransient"] = json!(true);
        cfg["privacy"]["lockScreen"] = json!("hide");
        let monitors: Value = serde_json::from_slice(&run(
            Command::new("hyprctl").args(["-j", "monitors"]),
            Duration::from_secs(5),
        )?)?;
        let m = monitors
            .as_array()
            .context("Invalid monitors")?
            .iter()
            .find(|m| m["name"] == monitor)
            .context("Unknown monitor")?;
        origin = "left-center";
        offset = format!(
            "({}, 55)",
            (m["width"].as_f64().unwrap_or(1000.) / m["scale"].as_f64().unwrap_or(1.) / 2. - 440.)
                .round()
                .max(0.)
        );
    }
    private_write(&path.join("config.json"), cfg.to_string().as_bytes())?;
    let template = include_str!("../examples/comparison-dunstrc");
    let dunst_cfg = template
        .replace(
            "@MONITOR@",
            &if monitor.is_empty() {
                "0".into()
            } else {
                json!(monitor).to_string()
            },
        )
        .replace("@ORIGIN@", origin)
        .replace("@OFFSET@", &offset)
        .replace("@WIDTH@", &cfg["style"]["width"].to_string())
        .replace(
            "@ICON@",
            &json!(app.join("qml/icons/vintage-robot.png")).to_string(),
        );
    private_write(&path.join("dunstrc"), dunst_cfg.as_bytes())?;
    let mut value = json!({"pid":std::process::id(),"birth":birth(std::process::id()),"buses":{},"app":app,"dunst":dunst,"monitor":monitor,"offsetX":cfg["placement"]["offsetX"],"width":cfg["style"]["width"],"signature":signature(root)?});
    let mut children = Children(vec![]);
    for name in ["dunst", "notjik"] {
        let mut bus = Command::new("dbus-daemon")
            .arg("--nofork")
            .arg(format!(
                "--config-file={}",
                root.join("examples/comparison-bus.conf").display()
            ))
            .arg("--print-address=1")
            .stdout(Stdio::piped())
            .spawn()?;
        let mut address = String::new();
        BufReader::new(bus.stdout.take().unwrap()).read_line(&mut address)?;
        children.0.push(bus);
        ensure!(!address.trim().is_empty(), "Failed to start private bus");
        value["buses"][name] = json!(address.trim());
        let mut cmd = if name == "dunst" {
            let mut c = Command::new(dunst);
            c.arg("-conf").arg(path.join("dunstrc"));
            c
        } else {
            let mut c = Command::new("quickshell");
            c.arg("--path").arg(app.join("qml")).arg("--no-color");
            c
        };
        environment(&mut cmd, &value, name);
        children.0.push(cmd.spawn()?);
    }
    let start = Instant::now();
    loop {
        ensure!(
            !STOP.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(12),
            "Comparison startup failed"
        );
        ensure!(
            children
                .0
                .iter_mut()
                .all(|c| c.try_wait().ok().flatten().is_none()),
            "Comparison child exited"
        );
        let ready = ["dunst", "notjik"].iter().all(|name| {
            run(
                Command::new("busctl")
                    .arg(format!("--address={}", text(&value["buses"][name])))
                    .args([
                        "call",
                        "org.freedesktop.DBus",
                        "/org/freedesktop/DBus",
                        "org.freedesktop.DBus",
                        "NameHasOwner",
                        "s",
                        "org.freedesktop.Notifications",
                    ]),
                Duration::from_secs(2),
            )
            .is_ok_and(|b| String::from_utf8_lossy(&b).contains("true"))
        });
        if ready {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    private_write(&path.join("session.json"), value.to_string().as_bytes())?;
    while !STOP.load(Ordering::Relaxed)
        && children
            .0
            .iter_mut()
            .all(|c| c.try_wait().ok().flatten().is_none())
    {
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}
pub fn send(v: &Value, args: &[String], replacements: Option<&Value>) -> Result<Value> {
    grouping(v, false)?;
    std::thread::scope(|scope| {
        let mut jobs = vec![];
        for name in ["dunst", "notjik"] {
            jobs.push((
                name,
                scope.spawn(move || -> Result<Value> {
                    let mut cmd = Command::new("notify-send");
                    cmd.arg("--print-id");
                    if let Some(r) = replacements {
                        cmd.arg("--replace-id").arg(r[name]["id"].to_string());
                    }
                    cmd.args(args);
                    environment(&mut cmd, v, name);
                    let out = String::from_utf8(run(&mut cmd, Duration::from_secs(120))?)?;
                    let mut lines = out.lines();
                    let id = lines
                        .next()
                        .context("Missing notification ID")?
                        .parse::<u64>()?;
                    Ok(json!({"id":id,"action":lines.collect::<Vec<_>>()}))
                }),
            ));
        }
        let mut result = json!({});
        for (name, job) in jobs {
            result[name] = job
                .join()
                .map_err(|_| anyhow::anyhow!("Comparison sender failed"))??;
        }
        Ok(result)
    })
}
pub fn clear(v: &Value) -> Result<()> {
    run(
        Command::new("busctl")
            .arg(format!("--address={}", text(&v["buses"]["dunst"])))
            .args([
                "call",
                "org.freedesktop.Notifications",
                "/org/freedesktop/Notifications",
                "org.dunstproject.cmd0",
                "NotificationCloseAll",
            ]),
        Duration::from_secs(5),
    )?;
    call(v, &json!({"op":"dismiss","id":"all"}))?;
    Ok(())
}
pub fn main(root: &Path, args: &[String]) -> Result<()> {
    let op = args.first().map(String::as_str).unwrap_or("help");
    let cases: Value = serde_json::from_str(include_str!("../examples/comparison-cases.json"))?;
    if op == "cases" {
        for k in cases.as_object().unwrap().keys() {
            println!("{k}")
        }
        println!("replace\nimage\nstack");
        return Ok(());
    }
    if op == "help" || op == "--help" {
        println!("notjikctl compare <start [--dunst PATH]|stop|status|clear|cases|case NAME|send -- ARGS>");
        return Ok(());
    }
    let value = if op == "start" {
        let _lock = control_lock()?;
        Some(start(
            root,
            args.windows(2)
                .find(|w| w[0] == "--dunst")
                .map(|w| w[1].as_str()),
        )?)
    } else if op == "stop" {
        let _lock = control_lock()?;
        stop()?;
        println!("Comparison stopped; desktop notification service unchanged");
        return Ok(());
    } else {
        state()
    };
    if op == "status" || op == "start" {
        let v = value.unwrap_or(Value::Null);
        emit(
            &json!({"running":!v.is_null(),"monitor":v["monitor"],"notjikOffsetX":v["offsetX"],"width":v["width"],"directory":directory()}),
        );
        return Ok(());
    }
    let v = value.context("Start the comparison first: notjikctl compare start")?;
    match op {
        "clear" => clear(&v)?,
        "send" => {
            let args = &args[if args.get(1).is_some_and(|s| s == "--") {
                2
            } else {
                1
            }..];
            ensure!(!args.is_empty(), "Pass notify-send arguments after --");
            emit(&send(&v, args, None)?);
        }
        "case" => {
            let name = args.get(1).context("Missing case name")?;
            let common = vec![
                "-a".into(),
                "Parity test".into(),
                "-t".into(),
                "12000".into(),
            ];
            let case = |tail: Vec<String>, replace: Option<&Value>| {
                send(&v, &[common.clone(), tail].concat(), replace)
            };
            let result = match name.as_str() {
                "replace" => {
                    let first = case(
                        vec![
                            "-h".into(),
                            "int:value:10".into(),
                            "Upload".into(),
                            "Initial 10 percent".into(),
                        ],
                        None,
                    )?;
                    std::thread::sleep(Duration::from_secs(1));
                    case(
                        vec![
                            "-h".into(),
                            "int:value:75".into(),
                            "Upload".into(),
                            "Same ID, updated to 75 percent".into(),
                        ],
                        Some(&first),
                    )?
                }
                "image" => case(
                    vec![
                        "-i".into(),
                        root.join("examples/demo-artwork.png")
                            .to_string_lossy()
                            .into(),
                        "Image artwork".into(),
                        "Synthetic bitmap image-data".into(),
                    ],
                    None,
                )?,
                "stack" => json!((1..=4)
                    .map(|i| case(
                        vec![
                            format!("Stack card {i}"),
                            "All visible cards should stay full-size".into()
                        ],
                        None
                    ))
                    .collect::<Result<Vec<_>>>()?),
                _ => case(
                    cases[name]
                        .as_array()
                        .context("Unknown comparison case")?
                        .iter()
                        .map(|v| text(v).to_string())
                        .collect(),
                    None,
                )?,
            };
            emit(&result);
        }
        _ => anyhow::bail!("Unknown comparison operation"),
    };
    Ok(())
}
