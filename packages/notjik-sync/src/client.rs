//! Shared durable producer and device transport. No credentials or records in Git.
use crate::*;
use anyhow::{bail, Context};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    io::{Read, Seek, Write},
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub url: String,
    #[serde(default)]
    pub socket: Option<PathBuf>,
    pub device: String,
    pub secret: String,
    pub name: String,
    pub kind: String,
    #[serde(default = "exclusions")]
    pub history_exclude: Vec<String>,
}
fn exclusions() -> Vec<String> {
    ["telegram", "Telegram via Notjik", "org.telegram.desktop"]
        .map(str::to_owned)
        .to_vec()
}
fn history_excluded(cfg: &Config, app: &str) -> bool {
    cfg.history_exclude.iter().any(|excluded| {
        excluded.eq_ignore_ascii_case(app)
            || (excluded.eq_ignore_ascii_case("telegram")
                && [
                    "Telegram Desktop",
                    "Telegram via Notjik",
                    "org.telegram.desktop",
                ]
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(app)))
    })
}
fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/".into())
}
pub fn config_path() -> PathBuf {
    if unsafe { libc::geteuid() } == 0
        && std::env::var_os("NOTJIK_SYNC_CONFIG").is_none()
        && Path::new("/etc/notjik/system-client.json").is_file()
    {
        return "/etc/notjik/system-client.json".into();
    }
    std::env::var_os("NOTJIK_SYNC_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".config"))
                .join("notjik/sync.json")
        })
}
pub fn state() -> PathBuf {
    if unsafe { libc::geteuid() } == 0
        && std::env::var_os("NOTJIK_SYNC_STATE").is_none()
        && Path::new("/etc/notjik/system-client.json").is_file()
    {
        return "/var/lib/notjik-system-client".into();
    }
    std::env::var_os("NOTJIK_SYNC_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".local/state"))
                .join("notjik/sync")
        })
}
pub fn configured() -> bool {
    config_path().is_file()
}
pub fn private_dir(p: &Path) -> Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(p)?;
    ensure!(
        !p.symlink_metadata()?.file_type().is_symlink(),
        "Private directory is a symlink"
    );
    Ok(())
}
pub fn atomic(p: &Path, v: &Value) -> Result<()> {
    private_dir(p.parent().context("Missing parent")?)?;
    let mut f = tempfile::NamedTempFile::new_in(p.parent().unwrap())?;
    serde_json::to_writer(&mut f, v)?;
    f.flush()?;
    f.as_file().sync_all()?;
    f.persist(p)?;
    Ok(())
}
pub fn config() -> Result<Config> {
    let p = config_path();
    ensure!(
        fs::metadata(&p)?.permissions().mode() & 0o077 == 0,
        "Sync configuration must be private"
    );
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
pub fn db() -> Result<Connection> {
    private_dir(&state())?;
    let c = Connection::open(state().join("client.sqlite3"))?;
    c.busy_timeout(std::time::Duration::from_secs(10))?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
CREATE TABLE IF NOT EXISTS outbox(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT UNIQUE,payload TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS commands(id TEXT PRIMARY KEY,notification TEXT NOT NULL,argv TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS received(id TEXT PRIMARY KEY,revision INTEGER NOT NULL,local_id TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS executions(id TEXT PRIMARY KEY,state TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS cache(id TEXT PRIMARY KEY,body TEXT NOT NULL);")?;
    Ok(c)
}
pub fn call(cfg: &Config, v: &Value) -> Result<Value> {
    let u = url::Url::parse(&cfg.url)?;
    ensure!(
        u.scheme() == "https" || cfg.socket.is_some(),
        "Notjik requires HTTPS"
    );
    ensure!(
        u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none(),
        "Invalid authority URL"
    );
    // Credentials live in an anonymous stdin-backed curl config, not argv.
    ensure!(
        !cfg.secret.contains(['\n', '\r', '"', '\\'])
            && !cfg.device.contains(['\n', '\r', '"', '\\']),
        "Invalid device credentials"
    );
    let mut input = tempfile::tempfile()?;
    writeln!(
        input,
        "header = \"X-Notjik-Device: {}\"\nheader = \"X-Notjik-Key: {}\"",
        cfg.device, cfg.secret
    )?;
    input.rewind()?;
    let mut body = tempfile::NamedTempFile::new()?;
    serde_json::to_writer(&mut body, v)?;
    body.flush()?;
    let mut cmd = Command::new("curl");
    cmd.args([
        "--silent",
        "--show-error",
        "--fail-with-body",
        "--max-time",
        "8",
        "--max-filesize",
        // History pages contain up to 100 envelopes of 64 KiB plus metadata.
        "16777216",
        "--config",
        "-",
        "--header",
        "Content-Type: application/json",
        "--header",
        "X-Notjik-Request: 1",
        "--data-binary",
    ]);
    cmd.arg(format!("@{}", body.path().display()));
    if let Some(socket) = &cfg.socket {
        cmd.arg("--unix-socket").arg(socket);
    }
    cmd.arg(format!("{}/api/v1/request", cfg.url.trim_end_matches('/')))
        .stdin(Stdio::from(input));
    let output = cmd.output()?;
    ensure!(
        output.status.success(),
        "Notification authority unavailable (transport or authentication failure)"
    );
    let v: Value = serde_json::from_slice(&output.stdout)?;
    ensure!(v.get("error").is_none(), "Notification request rejected");
    Ok(v)
}
pub fn enroll(url: String, name: String, kind: String, socket: Option<PathBuf>) -> Result<()> {
    ensure!(
        !configured(),
        "Device already configured; preserve its enrollment"
    );
    let cfg = Config {
        url,
        socket,
        device: id(),
        secret: format!("{}{}", id(), id()),
        name,
        kind,
        history_exclude: exclusions(),
    };
    // Save first: a failed response can be retried with the same identity.
    atomic(&config_path(), &serde_json::to_value(&cfg)?)?;
    call(
        &cfg,
        &json!({"op":"enroll","name":cfg.name,"kind":cfg.kind}),
    )?;
    Ok(())
}
fn queue(c: &Connection, mut v: Value) -> Result<()> {
    v["request_id"] = json!(id());
    c.execute(
        "INSERT INTO outbox(id,payload) VALUES(?1,?2)",
        params![text(&v, "request_id"), v.to_string()],
    )?;
    Ok(())
}
pub fn enqueue(v: Value) -> Result<()> {
    queue(&db()?, v)
}
pub fn mutation(mut v: Value) -> Result<()> {
    let c = db()?;
    v["request_id"] = json!(id());
    queue(&c, v)?;
    let _ = flush();
    Ok(())
}
pub fn publish(mut n: Value, delivery: &str, retain: bool) -> Result<String> {
    let cfg = config()?;
    let app = text(&n, "app").to_owned();
    let run = n["run"]
        .as_str()
        .or(n["id"].as_str())
        .unwrap_or("")
        .to_owned();
    let key = if run.is_empty() {
        id()
    } else {
        hash(&format!("{}:{app}:{run}", cfg.device))
    };
    n["id"] = json!(key);
    n["run"] = json!(run);
    n["delivery"] = json!(delivery);
    n["history"] = json!(retain && !history_excluded(&cfg, &app));
    n["shown"] = json!(false);
    if n["body"].is_null() {
        n["body"] = json!("");
    }
    let c = db()?;
    let mut actions = Vec::new();
    for a in n["actions"].as_array().cloned().unwrap_or_default() {
        if let Some(argv) = a["command"].as_array() {
            ensure!(
                !argv.is_empty() && argv.len() <= 128 && argv.iter().all(|x| x.is_string()),
                "Invalid command arguments"
            );
            let action_id = id();
            c.execute(
                "INSERT INTO commands VALUES(?1,?2,?3)",
                params![action_id, key, serde_json::to_string(argv)?],
            )?;
            actions.push(json!({"id":action_id,"label":a["label"]}));
        } else if a["url"].is_string() {
            actions.push(json!({"url":a["url"],"label":a["label"]}));
        }
    }
    n["actions"] = json!(actions);
    // Never send local media paths or process-local image handles to the server.
    n["image"] = json!("");
    if text(&n, "icon").contains('/') {
        n["icon"] = json!("");
    }
    n.as_object_mut().unwrap().remove("replace");
    validate(&n)?;
    if delivery == "local" {
        local(&n)?;
        n["shown"] = json!(true);
    }
    if n["history"] == true || delivery == "active" {
        queue(&c, json!({"op":"publish","notice":n}))?;
        let _ = flush();
    }
    Ok(key)
}
fn lock() -> Result<fs::File> {
    private_dir(&state())?;
    let f = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state().join("transport.lock"))?;
    ensure!(
        unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } == 0,
        "Transport lock failed"
    );
    Ok(f)
}
pub fn flush() -> Result<()> {
    let _guard = lock()?;
    let cfg = config()?;
    let c = db()?;
    let mut stmt = c.prepare("SELECT seq,payload FROM outbox ORDER BY seq LIMIT 100")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (seq, payload) in rows {
        let mut v: Value = serde_json::from_str(&payload)?;
        match call(&cfg, &v) {
            Ok(_) => {
                c.execute("DELETE FROM outbox WHERE seq=?1", [seq])?;
            }
            Err(error) => {
                if v["op"] == "publish"
                    && v["notice"]["shown"] != true
                    && std::env::var_os("WAYLAND_DISPLAY").is_some()
                {
                    // A timed-out publish may have committed. Reconcile before changing
                    // the request; idempotent event identity prevents duplicate history.
                    if local(&v["notice"]).is_ok() {
                        v["notice"]["shown"] = json!(true);
                        v["request_id"] = json!(id());
                        c.execute(
                            "UPDATE outbox SET id=?1,payload=?2 WHERE seq=?3",
                            params![text(&v, "request_id"), v.to_string(), seq],
                        )?;
                    }
                }
                return Err(error);
            }
        }
    }
    Ok(())
}
pub fn local(n: &Value) -> Result<String> {
    let mut display = n.clone();
    display["id"] = json!(format!("global-{}", text(n, "id")));
    display["globalId"] = n["id"].clone();
    display["actions"] = json!(n["actions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            if a["url"].is_string() {
                json!({"label":a["label"],"command":["xdg-open",a["url"]]})
            } else {
                json!({"label":a["label"],"command":[home().join(".local/bin/notjik-sync"),"invoke",n["id"],a["id"]]})
            }
        })
        .collect::<Vec<_>>());
    let mut f = tempfile::tempfile()?;
    serde_json::to_writer(&mut f, &display)?;
    f.rewind()?;
    let result = Command::new(home().join(".local/bin/notjikctl"))
        .arg("receive")
        .stdin(Stdio::from(f))
        .output()?;
    ensure!(result.status.success(), "Local Notjik unavailable");
    let handle = String::from_utf8(result.stdout)?.trim().to_owned();
    db()?.execute("INSERT INTO received VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET revision=MAX(received.revision,excluded.revision),local_id=excluded.local_id",params![text(n,"id"),n["revision"].as_i64().unwrap_or(0),handle])?;
    Ok(handle)
}
pub fn ipc(v: &Value) -> Result<Value> {
    let mut f = tempfile::NamedTempFile::new()?;
    serde_json::to_writer(&mut f, v)?;
    f.flush()?;
    let root = std::env::var_os("NOTJIK_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/lib/notjik"));
    let o = Command::new("timeout")
        .args(["3", "quickshell", "ipc", "--path"])
        .arg(root.join("qml"))
        .args(["call", "notjik", "requestFile"])
        .arg(f.path())
        .output()?;
    ensure!(o.status.success(), "Notjik IPC unavailable");
    let v: Value = serde_json::from_slice(&o.stdout)?;
    ensure!(v["ok"] == true, "Notjik IPC rejected request");
    Ok(v)
}
pub fn tick() -> Result<Value> {
    let cfg = config()?;
    let c = db()?;
    let status = ipc(&json!({"op":"status"})).unwrap_or(Value::Null);
    let ready = status["lockState"] == "unlocked";
    call(
        &cfg,
        &json!({"op":"presence","ready":ready,"paused":status["paused"]==true,"active":ready&&status["idle"]==false,"idle_seconds":status["idleSeconds"].as_i64().unwrap_or(120)}),
    )?;
    flush()?;
    let mut stmt = c.prepare("SELECT id FROM received ORDER BY rowid DESC LIMIT 1000")?;
    let ids = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let result = call(&cfg, &json!({"op":"poll","visible":ids}))?;
    for n in result["notices"].as_array().context("Missing notices")? {
        if !ready || status["paused"] == true {
            break;
        }
        let revision = n["revision"].as_i64().unwrap_or(1);
        let key = text(n, "id");
        let seen = c
            .query_row("SELECT revision FROM received WHERE id=?1", [key], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?;
        if seen.is_none_or(|r| r < revision) {
            let handle = local(n)?;
            c.execute("INSERT INTO received VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,local_id=excluded.local_id",params![key,revision,handle])?;
        }
        call(&cfg, &json!({"op":"ack","id":key,"revision":revision}))?;
    }
    for id in result["dismissed"]
        .as_array()
        .context("Missing dismissals")?
    {
        let handle = c
            .query_row(
                "SELECT local_id FROM received WHERE id=?1",
                [id.as_str().unwrap_or("")],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        if let Some(handle) = handle {
            ipc(&json!({"op":"global-remove","id":handle}))?;
            c.execute("DELETE FROM received WHERE id=?1", [id.as_str().unwrap()])?;
        }
    }
    for j in result["jobs"].as_array().context("Missing jobs")? {
        let argv = c
            .query_row(
                "SELECT argv FROM commands WHERE id=?1 AND notification=?2",
                params![text(j, "action"), text(j, "notification")],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        let claimed = call(&cfg, &json!({"op":"job-claim","id":j["id"]}))?;
        if claimed["claimed"] != true {
            continue;
        }
        // Persist before executing. A crash after this point is never auto-retried.
        let fresh = c.execute(
            "INSERT OR IGNORE INTO executions VALUES(?1,'started')",
            [text(j, "id")],
        )? == 1;
        let job_id = j["id"].clone();
        std::thread::spawn(move || {
            let exit = if fresh {
                argv.and_then(|a| serde_json::from_str::<Vec<String>>(&a).ok())
                    .and_then(|a| {
                        Command::new("timeout")
                            .arg("300")
                            .arg(&a[0])
                            .args(&a[1..])
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status()
                            .ok()
                    })
                    .and_then(|s| s.code())
            } else {
                None
            };
            if let Err(e) = mutation(
                json!({"op":"job-finish","id":job_id,"state":match exit{Some(0)=>"succeeded",Some(_)=>"failed",None=>"unknown"},"exit_code":exit}),
            ) {
                eprintln!("Action outcome queue failed: {e}");
            }
        });
    }
    // The drawer cache never publishes an event, so replay cannot loop.
    let history = call(&cfg, &json!({"op":"history"}))?;
    atomic(&state().join("history.json"), &history)?;
    atomic(&state().join("presence.json"), &result)?;
    Ok(result)
}
pub fn command(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str){
        Some("connect")=>{ensure!(args.len()>=4,"connect URL NAME desktop|server [SOCKET]");enroll(args[1].clone(),args[2].clone(),args[3].clone(),args.get(4).map(PathBuf::from))?;},
        Some("enroll")=>{let cfg=config()?;call(&cfg,&json!({"op":"enroll","name":cfg.name,"kind":cfg.kind}))?;},
        Some("run")=>loop{if let Err(e)=tick(){eprintln!("notjik-sync: {e}");}std::thread::sleep(std::time::Duration::from_secs(5));},
        Some("tick")=>{println!("{}",tick()?);},
        Some("request")=>{let mut input=String::new();std::io::stdin().take(65537).read_to_string(&mut input)?;ensure!(input.len()<=65536,"Request too large");println!("{}",call(&config()?,&serde_json::from_str(&input)?)?);},
        Some("publish")=>{let mut input=String::new();std::io::stdin().take(65537).read_to_string(&mut input)?;ensure!(input.len()<=65536,"Notice too large");println!("{}",publish(serde_json::from_str(&input)?,"active",true)?);},
        Some("invoke") if args.len()==3=>mutation(json!({"op":"invoke","id":args[1],"action":args[2]}))?,
        Some("dismiss"|"read") if args.len()==2=>mutation(json!({"op":args[0],"id":args[1]}))?,
        Some("history")=> {
            let query=args.get(1).cloned().unwrap_or_default();
            let offset=args.get(2).and_then(|s|s.parse::<usize>().ok()).unwrap_or(0);
            let result=match call(&config()?,&json!({"op":"history","query":query,"offset":offset})) {
                Ok(v)=>v,
                Err(_)=>{
                    let cache:Value=serde_json::from_slice(&fs::read(state().join("history.json"))?)?;
                    let terms=query.to_lowercase().split_whitespace().map(str::to_owned).collect::<Vec<_>>();
                    let matching=cache["notices"].as_array().context("Invalid cached history")?.iter().filter(|n|{let haystack=n.to_string().to_lowercase();terms.iter().all(|t|haystack.contains(t))}).skip(offset).take(100).cloned().collect::<Vec<_>>();
                    json!({"notices":matching,"next":null,"offline":true})
                }
            };println!("{result}");
        },
        Some("devices"|"jobs")=>println!("{}",call(&config()?,&json!({"op":args[0]}))?),
        Some("flush")=>flush()?,
        _=>bail!("notjik-sync connect URL NAME desktop|server [SOCKET] | enroll | run | tick | publish | history [QUERY] [OFFSET] | devices | jobs | invoke ID ACTION | dismiss ID | read ID")
    }
    Ok(())
}
