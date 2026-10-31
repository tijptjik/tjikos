//! Tasktjik adapter. Task ownership and recurrence rules remain in Tasktjik.
use crate::client_util::*;
use crate::model;
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    os::fd::AsRawFd,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub fn expand(value: &str) -> PathBuf {
    if value == "~" {
        home()
    } else if let Some(p) = value.strip_prefix("~/") {
        home().join(p)
    } else {
        PathBuf::from(value)
    }
}
fn lock(path: &Path) -> Result<fs::File> {
    let f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    ensure!(
        unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } == 0,
        "Cannot lock Tasktjik state"
    );
    Ok(f)
}
pub fn options(settings: &Value) -> Result<Value> {
    let mut result = json!({"authority":"si","directory":"~/.local/share/runtjik-todo","vault":"","ssh_command":["ssh"],"url":"","socket":""});
    let root = xdg("config").parent().unwrap().to_owned();
    let read = |path: PathBuf| -> Result<Value> {
        if !path.exists() {
            return Ok(json!({}));
        }
        Ok(serde_json::to_value(toml::from_str::<toml::Value>(
            &fs::read_to_string(path)?,
        )?)?)
    };
    let legacy = read(root.join("runtjik/runtjik.conf"))?;
    let dedicated = read(root.join("tasktjik/tasktjik.conf"))?;
    for layer in [
        &legacy["plugin_settings"]["Todo"],
        &settings["plugin_settings"]["Todo"],
        &dedicated["tasktjik"],
    ] {
        if let Some(map) = layer.as_object() {
            for (k, v) in map {
                result[k] = v.clone();
            }
        }
    }
    Ok(result)
}
fn device() -> Result<String> {
    let root = xdg("state").parent().unwrap().join("tasktjik");
    private_dir(&root)?;
    let _lock = lock(&root.join(".device.lock"))?;
    let path = root.join("device.json");
    if path.exists() {
        let value: Value = serde_json::from_slice(&fs::read(path)?)?;
        let id = s(&value["id"]);
        Uuid::parse_str(id)?;
        return Ok(id.into());
    }
    let id = Uuid::new_v4().to_string();
    let name = fs::read_to_string("/etc/hostname").unwrap_or_else(|_| "desktop".into());
    atomic(
        &path,
        &serde_json::to_vec(&json!({"id":id,"name":name.trim()}))?,
    )?;
    Ok(id)
}
fn base_url(value: &str) -> Result<url::Url> {
    let parsed = url::Url::parse(value)?;
    ensure!(
        parsed.scheme() == "https"
            && parsed.host_str().is_some_and(|h| h.ends_with(".ts.net"))
            && parsed.username().is_empty()
            && parsed.password().is_none()
            && parsed.query().is_none()
            && parsed.fragment().is_none()
            && parsed.port_or_known_default() == Some(443),
        "Use the HTTPS Tailscale hostname"
    );
    ensure!(
        parsed.path().trim_end_matches('/') == "/tasktjik",
        "Tasktjik URL must end in /tasktjik"
    );
    Ok(parsed)
}
fn response(text: &str) -> Result<Value> {
    ensure!(text.len() <= 8 * 1024 * 1024, "Tasktjik response too large");
    let value: Value = serde_json::from_str(text)?;
    if let Some(error) = value.get("error") {
        bail!("{}", error.as_str().unwrap_or("Tasktjik request failed"));
    }
    value
        .get("result")
        .cloned()
        .context("Invalid Tasktjik response")
}
fn api(options: &Value, mut request: Value) -> Result<Value> {
    if !["list", "capabilities"].contains(&request["action"].as_str().unwrap_or("list"))
        && request.get("request_id").is_none()
    {
        request["request_id"] = json!(Uuid::new_v4().to_string());
    }
    let value = http(options, "request", Some(&request))?;
    value
        .get("result")
        .cloned()
        .context("Invalid Tasktjik response")
}
pub fn http(options: &Value, endpoint: &str, request: Option<&Value>) -> Result<Value> {
    let mut args: Vec<String> = vec![
        "curl",
        "--disable",
        "--silent",
        "--show-error",
        "--noproxy",
        "*",
        "--max-time",
        "10",
        "--max-filesize",
        "8388608",
        "--request",
        if request.is_some() { "POST" } else { "GET" },
        "--header",
        "Content-Type: application/json",
        "--header",
        "X-Tasktjik-Request: 1",
        "--header",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    args.push(format!("X-Tasktjik-Device: {}", device()?));
    if request.is_some() {
        args.extend(["--data-binary".into(), "@-".into()]);
    }
    args.extend(["--write-out".into(), "\n%{http_code}".into()]);
    let socket = s(&options["socket"]);
    let parsed = if socket.is_empty() {
        Some(base_url(s(&options["url"]))?)
    } else {
        None
    };
    if !socket.is_empty() {
        args.extend(["--unix-socket".into(), socket.into()]);
    }
    let endpoint = parsed
        .as_ref()
        .map(|p| format!("{}/api/v1/{endpoint}", p.as_str().trim_end_matches('/')))
        .unwrap_or_else(|| format!("http://localhost/api/v1/{endpoint}"));
    args.push(endpoint);
    let body = request.map(Value::to_string).unwrap_or_default();
    let output = match run(&args, Some(&body), 12) {
        Ok(v) => v,
        Err(e) if parsed.is_some() && e.to_string().contains("(6)") => {
            let host = parsed.as_ref().unwrap().host_str().unwrap();
            let status: Value =
                serde_json::from_str(&command(&["tailscale", "status", "--json"])?)?;
            let peer = std::iter::once(&status["Self"])
                .chain(
                    status["Peer"]
                        .as_object()
                        .into_iter()
                        .flat_map(|m| m.values()),
                )
                .find(|p| s(&p["DNSName"]).trim_end_matches('.') == host)
                .context("Tasktjik host is not present in the local tailnet")?;
            let ip: std::net::IpAddr = s(&peer["TailscaleIPs"][0]).parse()?;
            args.extend([
                "--resolve".into(),
                format!(
                    "{host}:443:{}",
                    if ip.is_ipv6() {
                        format!("[{ip}]")
                    } else {
                        ip.to_string()
                    }
                ),
            ]);
            run(&args, Some(&body), 12)?
        }
        Err(e) => return Err(e),
    };
    let (body, code) = output.rsplit_once('\n').context("Missing HTTP status")?;
    if code != "200" {
        if let Ok(v) = serde_json::from_str::<Value>(body) {
            if let Some(e) = v["error"].as_str() {
                bail!("{e}");
            }
        }
        bail!("Tasktjik HTTP status {code}");
    }
    let value: Value = serde_json::from_str(body)?;
    if let Some(error) = value.get("error") {
        bail!("{}", error.as_str().unwrap_or("Tasktjik request failed"));
    }
    Ok(value)
}

#[derive(Default)]
pub struct Todo {
    demo_tasks: Vec<Value>,
}
fn apply(tasks: &mut Vec<Value>, request: &Value) -> Result<Value> {
    let action = request["action"].as_str().unwrap_or("list");
    for t in tasks.iter_mut() {
        model::defaults(t);
    }
    if action == "list" {
        return Ok(json!(tasks));
    }
    if action == "capabilities" {
        return Ok(
            json!({"reading":true,"idempotent_create":true,"agent_tasks":true,"protocol":1}),
        );
    }
    if action == "create" {
        if let Some(key) = request["request_id"].as_str() {
            Uuid::parse_str(key)?;
            if let Some(t) = tasks.iter().find(|t| t["createKey"] == key) {
                return Ok(t.clone());
            }
        }
    }
    if ["status", "verify"].contains(&action) {
        ensure!(!s(&request["actor"]).trim().is_empty(), "Actor is required");
    }
    let result = model::mutate(tasks, request)?;
    if action == "purge" {
        tasks.retain(|t| t["id"] != result["id"]);
    } else if result.is_object() {
        if let Some(t) = tasks.iter_mut().find(|t| t["id"] == result["id"]) {
            *t = result.clone();
        } else {
            tasks.push(result.clone());
        }
        tasks.sort_by_key(|t| s(&t["id"]).to_owned());
    }
    Ok(result)
}
impl Todo {
    pub fn request(&mut self, request: Value, settings: &Value, demo: bool) -> Result<Value> {
        if demo {
            return apply(&mut self.demo_tasks, &request);
        }
        let options = options(settings)?;
        if !s(&options["url"]).is_empty() || !s(&options["socket"]).is_empty() {
            return api(&options, request);
        }
        let host = s(&options["authority"]);
        if !host.is_empty() {
            ensure!(
                !host.starts_with('-')
                    && host
                        .chars()
                        .all(|c| c.is_alphanumeric() || ".-@_".contains(c)),
                "Invalid authority SSH host"
            );
            let mut args = strings(&options["ssh_command"]);
            ensure!(
                !args.is_empty()
                    && args.iter().all(|s| !s.is_empty())
                    && options["ssh_command"]
                        .as_array()
                        .is_some_and(|a| a.len() == args.len()),
                "Todo.ssh_command must be a nonempty argv array"
            );
            args.extend(
                [
                    "-oBatchMode=yes",
                    "-oStrictHostKeyChecking=yes",
                    "-oConnectTimeout=4",
                    host,
                    ".local/lib/tasktjik/tasktjik-api serve",
                ]
                .into_iter()
                .map(str::to_owned),
            );
            let mut body = json!({"directory":options["directory"]});
            for (k, v) in request.as_object().context("Invalid request")? {
                body[k] = v.clone();
            }
            return response(&run(&args, Some(&body.to_string()), 10)?);
        }
        self.local(request, s(&options["directory"]))
    }
    pub fn local(&mut self, request: Value, directory: &str) -> Result<Value> {
        let root = expand(directory);
        // Resolve ancestors before creating directories so repository symlinks cannot bypass this guard.
        let mut parent = root.as_path();
        let mut suffix = Vec::new();
        while !parent.exists() {
            suffix.push(
                parent
                    .file_name()
                    .context("Invalid task directory")?
                    .to_owned(),
            );
            parent = parent.parent().context("Invalid task directory")?;
        }
        let mut root = fs::canonicalize(parent)?;
        for p in suffix.iter().rev() {
            root.push(p);
        }
        let app = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        ensure!(
            !root.starts_with(&app),
            "Todo data must be outside the application directory"
        );
        for ancestor in app.ancestors() {
            if ancestor.join(".git").exists() {
                ensure!(
                    !root.starts_with(ancestor),
                    "Todo data must be outside the repository"
                );
                break;
            }
        }
        private_dir(&root)?;
        let _lock = lock(&root.join(".lock"))?;
        let mut paths = fs::read_dir(&root)?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        let mut tasks = Vec::new();
        for p in paths {
            if p.extension().is_some_and(|e| e == "json") {
                tasks.push(serde_json::from_slice(&fs::read(p)?)?);
            }
        }
        let result = apply(&mut tasks, &request)?;
        if result.is_object() && request["action"] != "capabilities" {
            let id = s(&result["id"]);
            Uuid::parse_str(id)?;
            if request["action"] == "purge" {
                fs::remove_file(root.join(format!("{id}.json")))?;
            } else {
                atomic(
                    &root.join(format!("{id}.json")),
                    &serde_json::to_vec_pretty(&result)?,
                )?;
            }
            fs::File::open(&root)?.sync_all()?;
        }
        Ok(result)
    }
    pub fn state(&mut self, request: Value, settings: &Value, demo: bool) -> Result<Value> {
        let action = request["action"].as_str().unwrap_or("list");
        if action == "plans" {
            if demo {
                return Ok(
                    json!({"plans":["Projects/Synthetic plan.md","Home/Synthetic chores.md"]}),
                );
            }
            let opts = options(settings)?;
            if s(&opts["vault"]).is_empty() {
                return Ok(json!({"plans":[],"message":"Configure tasktjik.vault"}));
            }
            let base = fs::canonicalize(expand(s(&opts["vault"])))?;
            let mut paths: Vec<_> = walkdir::WalkDir::new(&base)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| {
                    e.path().extension().is_some_and(|s| s == "md")
                        && e.path().is_file()
                        && fs::canonicalize(e.path()).is_ok_and(|p| p.starts_with(&base))
                })
                .take(10000)
                .filter_map(|e| {
                    e.path()
                        .strip_prefix(&base)
                        .ok()
                        .map(|p| p.to_string_lossy().into_owned())
                })
                .collect();
            paths.sort();
            return Ok(json!({"plans":paths}));
        }
        if action != "list" && action != "open-reference" {
            self.request(request.clone(), settings, demo)?;
        }
        let mut rows = self.request(json!({"action":"list"}), settings, demo)?;
        let rows = rows.as_array_mut().context("Invalid Tasktjik task list")?;
        if action == "open-reference" {
            let t = rows
                .iter()
                .find(|t| t["id"] == request["id"] && t["kind"] == "reading")
                .context("Select a reading item")?;
            let url = url::Url::parse(s(&t["reference"]))?;
            ensure!(
                ["http", "https"].contains(&url.scheme()) && url.host_str().is_some(),
                "Reading items need an HTTP or HTTPS reference"
            );
            if !demo {
                dispatch(vec!["xdg-open".into(), url.into()], &json!({}))?;
            }
            return Ok(json!({"message":"Opened reading reference"}));
        }
        let mut groups: Vec<String> = [
            "All", "Quick", "General", "Work", "Socials", "Domestic", "Finances", "Tech",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let mut extra = Vec::new();
        for t in rows.iter_mut() {
            let mut recurrence = t.clone();
            if !s(&recurrence["repeat"]["unit"]).is_empty()
                && recurrence["repeat"].get("value").is_none()
            {
                recurrence["repeat"]["value"] = json!(1);
            }
            let period = model::period(&recurrence, model::today())?;
            t["done"] = json!(t["completions"]
                .get(&period)
                .is_some_and(|v| !v.is_null() && v != false && v != ""));
            t["period"] = json!(period);
            extra.push(s(&t["group"]).to_owned());
        }
        extra.sort_by_key(|s| s.to_lowercase().replace('ß', "ss"));
        for g in extra {
            if !groups.contains(&g) {
                groups.push(g);
            }
        }
        Ok(json!({"tasks":rows,"groups":groups,"today":model::today().to_string()}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_tailnet_urls() {
        assert!(base_url("https://si.example.ts.net/tasktjik/").is_ok());
        for url in [
            "http://si.example.ts.net/tasktjik",
            "https://example.com/tasktjik",
            "https://user@si.x.ts.net/tasktjik",
            "https://si.x.ts.net:444/tasktjik",
            "https://si.x.ts.net/tasktjik?q=a",
            "https://si.x.ts.net/other",
        ] {
            assert!(base_url(url).is_err(), "{url}");
        }
    }
    #[test]
    fn demo_mutations_are_idempotent_and_revision_guarded() {
        let mut todo = Todo::default();
        let key = Uuid::new_v4().to_string();
        let create = json!({"action":"create","request_id":key,"fields":{"title":"Synthetic reading", "kind":"reading","reference":"https://example.com"}});
        let task = todo.request(create.clone(), &json!({}), true).unwrap();
        assert_eq!(todo.request(create, &json!({}), true).unwrap(), task);
        assert!(todo
            .request(
                json!({"action":"edit","id":task["id"],"revision":0,"fields":{"title":"stale"}}),
                &json!({}),
                true
            )
            .is_err());
        let state = todo
            .state(
                json!({"action":"toggle","id":task["id"],"revision":task["revision"]}),
                &json!({}),
                true,
            )
            .unwrap();
        assert_eq!(state["tasks"][0]["done"], true);
        assert_eq!(state["tasks"].as_array().unwrap().len(), 1);
    }
}

pub fn connect(url: &str, notifications: bool) -> Result<Value> {
    base_url(url)?;
    let url = url.trim_end_matches('/');
    let opts = json!({"url":url,"socket":""});
    let identity = http(&opts, "identity", None)?;
    let id = device()?;
    let mut record: Value = serde_json::from_slice(&fs::read(
        xdg("state").parent().unwrap().join("tasktjik/device.json"),
    )?)?;
    record["id"] = json!(id);
    record["notifications"] = json!(notifications);
    http(&opts, "devices", Some(&record))?;
    let root = xdg("config").parent().unwrap().join("tasktjik");
    private_dir(&root)?;
    let _lock = lock(&root.join(".config.lock"))?;
    let path = root.join("tasktjik.conf");
    let source = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let mut doc = source.parse::<toml_edit::DocumentMut>()?;
    doc["tasktjik"]["url"] = toml_edit::value(url);
    // Explicit enrollment selects HTTPS even if the machine previously used a socket.
    doc["tasktjik"]["socket"] = toml_edit::value("");
    if path.exists() {
        atomic(
            &path.with_extension(format!("conf.pre-api-{}", Uuid::new_v4())),
            source.as_bytes(),
        )?;
    }
    atomic(&path, doc.to_string().as_bytes())?;
    Ok(json!({"connected":url,"identity":identity["login"],"device":record}))
}

fn send_notice(task: &Value) -> Result<()> {
    let root = std::env::var_os("NOTJIK_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/lib/notjik"));
    let payload = json!({"op":"send","notice":{
        "title":task["title"],"body":"","app":"tasktjik","type":"todo","urgency":"normal","timeout":0,
        "id":task["id"],"icon":"","image":"","resident":false,"transient":false,"actions":[],
        "data":{"group":task["group"],"due":task["period"],"taskId":task["id"],"period":task["period"]}
    }});
    let bytes = serde_json::to_vec(&payload)?;
    ensure!(bytes.len() <= 131072, "Notification exceeds 128 KiB");
    let directory = tempfile::Builder::new()
        .prefix("tasktjik-notice-")
        .tempdir()?;
    let path = directory.path().join("request.json");
    atomic(&path, &bytes)?;
    let result: Value = serde_json::from_str(&command(&[
        "quickshell",
        "ipc",
        "--path",
        &root.join("qml").to_string_lossy(),
        "call",
        "notjik",
        "requestFile",
        &path.to_string_lossy(),
    ])?)?;
    ensure!(
        result["ok"] == true,
        "{}",
        result["error"]
            .as_str()
            .unwrap_or("Notification delivery failed")
    );
    Ok(())
}

pub fn notify() -> Result<Value> {
    use chrono::{Duration, Local, TimeZone};
    let mut todo = Todo::default();
    let settings = json!({});
    let state = todo.state(json!({}), &settings, false)?;
    let device = device()?;
    let mut delivered = 0;
    for mut task in state["tasks"]
        .as_array()
        .context("Invalid task list")?
        .iter()
        .cloned()
    {
        if s(&task["due"]).is_empty() || task["status"] == "cancelled" {
            continue;
        }
        if task["done"] == true {
            if s(&task["repeat"]["unit"]).is_empty() {
                continue;
            }
            task["period"] = json!(model::next_period(&task, s(&task["period"]))?);
        }
        let date = chrono::NaiveDate::parse_from_str(s(&task["period"]), "%Y-%m-%d")?;
        let due = Local
            .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
            .earliest()
            .context("Invalid local due date")?;
        let value = task["notifyBeforeDueLeadValue"].as_i64().unwrap_or(1);
        ensure!((0..=10000).contains(&value), "Invalid notification lead");
        let lead = if task["notifyBeforeDue"] != true {
            Duration::zero()
        } else {
            match s(&task["notifyBeforeDueLeadUnit"]) {
                "hours" => Duration::hours(value),
                "days" => Duration::days(value),
                "weeks" => Duration::weeks(value),
                _ => bail!("Invalid notification lead unit"),
            }
        };
        if Local::now() < due - lead {
            continue;
        }
        let base = json!({"id":task["id"],"period":task["period"],"device":device});
        let mut request = base.clone();
        request["action"] = json!("claim");
        let claim = todo.request(request, &settings, false)?;
        if claim == false {
            continue;
        }
        let (key, reservation) = claim["notifications"]
            .as_object()
            .context("Missing notification lease")?
            .iter()
            .filter(|(k, v)| {
                k.starts_with(&format!("{}:", s(&task["period"])))
                    && v["state"] == "pending"
                    && v["device"] == device
            })
            .max_by_key(|(k, _)| *k)
            .context("Missing notification lease")?;
        let mut finish = base;
        finish["token"] = reservation["token"].clone();
        finish["notification_key"] = json!(key);
        if let Err(error) = send_notice(&task) {
            finish["action"] = json!("release");
            todo.request(finish, &settings, false)?;
            return Err(error);
        }
        finish["action"] = json!("ack");
        todo.request(finish, &settings, false)?;
        delivered += 1;
    }
    Ok(json!({"delivered":delivered}))
}

/// Client subcommands return false only when arguments belong to the authority.
pub fn command_line(args: &[String]) -> Result<bool> {
    use std::io::Read;
    let Some(cmd) = args.first().map(String::as_str) else {
        return Ok(false);
    };
    let read = || -> Result<Value> {
        let mut text = String::new();
        std::io::stdin()
            .take(8 * 1024 * 1024 + 1)
            .read_to_string(&mut text)?;
        ensure!(text.len() <= 8 * 1024 * 1024, "Request too large");
        Ok(serde_json::from_str(&text)?)
    };
    let mut todo = Todo::default();
    let settings = json!({});
    let result = match cmd {
        "state" => todo.state(read()?, &settings, args.iter().any(|a| a == "--demo"))?,
        "request" => todo.request(read()?, &settings, args.iter().any(|a| a == "--demo"))?,
        "serve" => {
            let mut request = read()?;
            let directory = request["directory"]
                .as_str()
                .unwrap_or("~/.local/share/runtjik-todo")
                .to_owned();
            request
                .as_object_mut()
                .context("Request must be an object")?
                .remove("directory");
            match todo.local(request, &directory) {
                Ok(v) => json!({"result":v}),
                Err(e) => json!({"error":e.to_string()}),
            }
        }
        "connect" => connect(
            args.get(1)
                .context("Usage: tasktjik connect URL [--notifications]")?,
            args.iter().any(|s| s == "--notifications"),
        )?,
        "devices" => http(&options(&settings)?, "devices", None)?,
        "notify" => notify()?,
        "done" | "dismiss" | "snooze" => {
            let mut r = json!({"action":cmd,"id":args.get(1).context("Task ID required")?,"period":args.get(2).context("Task period required")?});
            if cmd == "snooze" {
                r["until"] = json!(args.get(3).context("Snooze time required")?);
            }
            todo.request(r, &settings, false)?
        }
        _ => return Ok(false),
    };
    println!("{result}");
    Ok(true)
}
