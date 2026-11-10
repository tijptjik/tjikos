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
pub(crate) fn lock(path: &Path) -> Result<fs::File> {
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
pub(crate) fn device() -> Result<String> {
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
#[derive(Debug)]
pub struct Rejected(pub String);
impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Rejected {}
fn response(text: &str) -> Result<Value> {
    ensure!(text.len() <= 8 * 1024 * 1024, "Tasktjik response too large");
    let value: Value = serde_json::from_str(text)?;
    if let Some(error) = value.get("error") {
        return Err(Rejected(error.as_str().unwrap_or("Tasktjik request failed").into()).into());
    }
    value
        .get("result")
        .cloned()
        .context("Invalid Tasktjik response")
}
pub(crate) fn api(options: &Value, mut request: Value) -> Result<Value> {
    if !["list", "capabilities", "image-get"]
        .contains(&request["action"].as_str().unwrap_or("list"))
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
                return Err(Rejected(e.into()).into());
            }
        }
        bail!("Tasktjik HTTP status {code}");
    }
    let value: Value = serde_json::from_str(body)?;
    if let Some(error) = value.get("error") {
        return Err(Rejected(error.as_str().unwrap_or("Tasktjik request failed").into()).into());
    }
    Ok(value)
}

#[derive(Default)]
pub struct Todo {
    demo_tasks: Vec<Value>,
    demo_images: std::collections::HashMap<String, Vec<u8>>,
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
            json!({"reading":true,"idempotent_create":true,"agent_tasks":true,"metadata":true,"images":true,"projects":true,"workflows":true,"undated_repeat":true,"groups":true,"workflow_dependencies":true,"conditional_pathways":true,"protocol":1}),
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
    if let Some(changed) = result["changed"].as_array() {
        crate::groups::apply(tasks, changed);
    } else if action == "purge" {
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
            if request["action"] == "image-get" {
                let metadata = crate::images::attached(&self.demo_tasks, &request)?;
                return Ok(crate::images::source(
                    metadata,
                    self.demo_images
                        .get(s(&metadata["id"]))
                        .context("Image unavailable")?,
                ));
            }
            let result = apply(&mut self.demo_tasks, &request)?;
            if request["action"] == "attach-image" {
                let (metadata, bytes) = crate::images::decode(s(&request["data"]))?;
                self.demo_images.insert(s(&metadata["id"]).into(), bytes);
            }
            return Ok(result);
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
        // Roll forward an interrupted batch before exposing any local snapshot.
        let journal = root.join(".group-batch");
        if journal.exists() {
            let changes: Vec<Value> = serde_json::from_slice(&fs::read(&journal)?)?;
            for task in changes {
                let id = s(&task["id"]);
                Uuid::parse_str(id)?;
                atomic(
                    &root.join(format!("{id}.json")),
                    &serde_json::to_vec(&task)?,
                )?;
            }
            fs::remove_file(&journal)?;
            fs::File::open(&root)?.sync_all()?;
        }
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
        if request["action"] == "image-get" {
            let metadata = crate::images::attached(&tasks, &request)?;
            let id = s(&metadata["id"]);
            ensure!(
                id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()),
                "Invalid image ID"
            );
            return Ok(crate::images::source(
                metadata,
                &fs::read(root.join("images").join(id))?,
            ));
        }
        let result = apply(&mut tasks, &request)?;
        if request["action"] == "attach-image" {
            let (metadata, bytes) = crate::images::decode(s(&request["data"]))?;
            atomic(&root.join("images").join(s(&metadata["id"])), &bytes)?;
        }
        if let Some(changed) = result["changed"].as_array() {
            atomic(&journal, &serde_json::to_vec(changed)?)?;
            for task in changed {
                let id = s(&task["id"]);
                Uuid::parse_str(id)?;
                atomic(&root.join(format!("{id}.json")), &serde_json::to_vec(task)?)?;
            }
            fs::remove_file(&journal)?;
            fs::File::open(&root)?.sync_all()?;
        } else if result.is_object() && request["action"] != "capabilities" {
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
        let rows = self.request(json!({"action":"list"}), settings, demo)?;
        render_state(rows, &request, demo)
    }

    /// UI clients already have a list. Reconcile a mutation from its acknowledged
    /// task instead of making a second network request for the entire list.
    pub fn state_delta(&mut self, request: Value, settings: &Value, demo: bool) -> Result<Value> {
        let action = request["action"].as_str().unwrap_or("list");
        if ![
            "create",
            "edit",
            "toggle",
            "done",
            "status",
            "snooze",
            "verify",
            "purge",
            "move",
            "undo",
            "attach-image",
            "subtask-toggle",
            "subtask-cancel",
        ]
        .contains(&action)
        {
            return self.state(request, settings, demo);
        }
        let task = self.request(request.clone(), settings, demo)?;
        if task["changed"].is_array() {
            let rows = self.request(json!({"action":"list"}), settings, demo)?;
            let mut state = render_state(rows, &request, demo)?;
            state["mutation_accepted"] = json!(true);
            return Ok(state);
        }
        ensure!(task["id"].is_string(), "Missing acknowledged task ID");
        let removed = if task["deleted"] == true {
            json!([task["id"]])
        } else {
            json!([])
        };
        let rows = if task["deleted"] == true {
            json!([])
        } else {
            json!([task])
        };
        let mut state = render_state(rows, &request, demo)?;
        state["groups"] = json!(state["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["group"].as_str())
            .collect::<Vec<_>>());
        state["incremental"] = json!(true);
        state["removed"] = removed;
        state["mutation_accepted"] = json!(true);
        Ok(state)
    }
}

/// Derive UI fields from a complete snapshot without contacting the authority.
pub fn render_state(mut rows: Value, request: &Value, demo: bool) -> Result<Value> {
    let action = request["action"].as_str().unwrap_or("list");
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
    let mut groups = crate::groups::names(rows);
    let groups_revision = crate::groups::revision(rows);
    rows.retain(|t| t["kind"] != "settings");
    let mut extra = Vec::new();
    for t in rows.iter_mut() {
        model::defaults(t);
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
        crate::workflow::decorate(t, model::today())?;
        extra.push(s(&t["group"]).to_owned());
    }
    extra.sort_by_key(|s| s.to_lowercase().replace('ß', "ss"));
    for g in extra {
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    Ok(
        json!({"tasks":rows,"groups":groups,"groupsRevision":groups_revision,"today":model::today().to_string()}),
    )
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
    let mut payload = json!({"op":"send","notice":{
        "title":task["title"],"body":"","app":"tasktjik","type":"todo","urgency":"normal","timeout":0,
        "id":task["id"],"icon":"","image":"","resident":false,"transient":false,"actions":[],
        "data":{"group":task["group"],"due":task["period"],"taskId":task["id"],"period":task["period"]}
    }});
    let executable = home().join(".local/bin/tasktjik");
    let reminder = task["subtaskId"].is_string();
    if reminder {
        // Standard action cards also work on clients predating subtask-aware Todo cards.
        payload["notice"]["type"] = json!("standard");
        payload["notice"]["body"] = json!(format!("Reminder due {}", s(&task["period"])));
        payload["notice"]["id"] = json!(format!("{}:{}", s(&task["id"]), s(&task["subtaskId"])));
        payload["notice"]["data"]["subtaskId"] = task["subtaskId"].clone();
        payload["notice"]["actions"] = json!([
            {"label":"Mark reminder done","command":[executable,"subtask-done",task["id"],task["subtaskId"]]},
            {"label":"Dismiss reminder","command":[executable,"subtask-dismiss",task["id"],task["subtaskId"],task["period"]]}
        ]);
    }
    if notjik_sync::client::configured() {
        let mut notice = payload["notice"].clone();
        notice["run"] = json!(format!("{}:{}", s(&notice["id"]), s(&task["period"])));
        let executable = home().join(".local/bin/tasktjik");
        if !reminder {
            notice["actions"] = json!([
                {"label":"Mark done","command":[executable,"done",task["id"],task["period"]]},
                {"label":"Dismiss reminder","command":[executable,"dismiss",task["id"],task["period"]]}
            ]);
        }
        notjik_sync::client::publish(notice, "active", true)?;
        return Ok(());
    }
    notjik_sync::client::ipc(&payload)?;
    Ok(())
}

fn reminder_at(task: &Value) -> Result<Option<chrono::DateTime<chrono::Local>>> {
    use chrono::{Duration, Local, TimeZone};
    if task["status"] == "cancelled" {
        return Ok(None);
    }
    if task["done"] != true {
        if let Ok(until) = chrono::DateTime::parse_from_rfc3339(s(&task["snoozedUntil"])) {
            // A snooze is itself a reminder, including for an undated task.
            // Never apply the original due-date lead time to an explicit snooze.
            return Ok(Some(until.with_timezone(&Local)));
        }
    }
    if s(&task["due"]).is_empty() && s(&task["repeat"]["unit"]).is_empty() {
        return Ok(None);
    }
    let day = chrono::NaiveDate::parse_from_str(s(&task["period"]), "%Y-%m-%d")?;
    let due = Local
        .from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
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
    Ok(Some(due - lead))
}
#[cfg(test)]
mod snooze_reminder_tests {
    use super::*;
    #[test]
    fn undated_snoozes_wake_at_requested_instant_without_due_lead() {
        let task = json!({"period":"once","due":"","repeat":{},"snoozedUntil":"2026-11-10T20:00:00+08:00","notifyBeforeDue":true,"notifyBeforeDueLeadValue":7,"notifyBeforeDueLeadUnit":"days"});
        assert_eq!(
            reminder_at(&task).unwrap().unwrap().to_utc().to_rfc3339(),
            "2026-11-10T12:00:00+00:00"
        );
        let mut plain = task.clone();
        plain["snoozedUntil"] = Value::Null;
        assert!(reminder_at(&plain).unwrap().is_none());
        plain = task.clone();
        plain["status"] = json!("cancelled");
        assert!(reminder_at(&plain).unwrap().is_none());
        plain = task;
        plain["done"] = json!(true);
        assert!(reminder_at(&plain).unwrap().is_none());
    }
}
pub fn notify() -> Result<Value> {
    use chrono::Local;
    let mut todo = Todo::default();
    let settings = json!({});
    let state = todo.state(json!({}), &settings, false)?;
    let device = device()?;
    let mut delivered = 0;
    let mut candidates = Vec::new();
    for task in state["tasks"].as_array().context("Invalid task list")? {
        if ["project", "template"].contains(&s(&task["kind"])) {
            continue;
        }
        candidates.push(task.clone());
        if task["done"] == true || task["status"] == "cancelled" {
            continue;
        }
        for row in task["workflowRows"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["kind"] == "reminder" && r["status"] == "ready")
        {
            let mut reminder = task.clone();
            reminder["title"] = json!(format!("{} · {}", s(&task["title"]), s(&row["title"])));
            reminder["subtaskId"] = row["id"].clone();
            reminder["due"] = row["due"].clone();
            reminder["period"] = row["due"].clone();
            reminder["notifyBeforeDue"] = json!(false);
            candidates.push(reminder);
        }
    }
    for mut task in candidates {
        if task["done"] == true {
            if s(&task["repeat"]["unit"]).is_empty() {
                continue;
            }
            task["period"] = json!(model::next_period(&task, s(&task["period"]))?);
        }
        let Some(ready_at) = reminder_at(&task)? else {
            continue;
        };
        if Local::now() < ready_at {
            continue;
        }
        let base = json!({"id":task["id"],"period":task["period"],"device":device,"subtask_id":task["subtaskId"]});
        let prefix = if task["subtaskId"].is_string() {
            format!("subtask:{}:{}:", s(&task["subtaskId"]), s(&task["period"]))
        } else {
            format!("{}:", s(&task["period"]))
        };
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
                k.starts_with(&prefix) && v["state"] == "pending" && v["device"] == device
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
        "clipboard-image" => crate::images::clipboard()?,
        "state" if args.iter().any(|a| a == "--incremental") => {
            todo.state_delta(read()?, &settings, args.iter().any(|a| a == "--demo"))?
        }
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
        "enroll" => {
            let id = device()?;
            let record = json!({"id":id,"name":fs::read_to_string("/etc/hostname").unwrap_or_else(|_|"device".into()).trim(),"notifications":args.iter().any(|s|s=="--notifications")});
            http(&options(&settings)?, "devices", Some(&record))?
        }
        "devices" => http(&options(&settings)?, "devices", None)?,
        "notify" => notify()?,
        "subtask-done" => {
            let id = args.get(1).context("Task ID required")?;
            let subtask = args.get(2).context("Subtask ID required")?;
            let list = todo.request(json!({"action":"list"}), &settings, false)?;
            let task = list.as_array().context("Invalid task list")?.iter().find(|t| t["id"] == *id).context("Task no longer exists")?;
            let rows = crate::workflow::task_rows(&task, model::today())?;
            let row = rows.iter().find(|r| r["id"] == *subtask).context("Subtask is not active")?;
            if row["status"] == "done" { task.clone() } else {
                todo.request(json!({"action":"subtask-toggle","id":id,"subtask_id":subtask,"revision":task["revision"]}), &settings, false)?
            }
        }
        "subtask-dismiss" => todo.request(json!({"action":"dismiss","id":args.get(1).context("Task ID required")?,"subtask_id":args.get(2).context("Subtask ID required")?,"period":args.get(3).context("Reminder date required")?}), &settings, false)?,
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
