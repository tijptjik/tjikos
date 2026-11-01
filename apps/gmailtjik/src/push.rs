//! Pub/Sub is an untrusted wake-up signal. Gmail history remains authoritative.
use crate::{auth, store::Store, worker};
use anyhow::{Context, Result, ensure};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub enabled: bool,
    pub topic: String,
    pub subscription: String,
    pub credentials_file: PathBuf,
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        fn resource(value: &str, kind: &str) -> Option<String> {
            let parts: Vec<_> = value.split('/').collect();
            (parts.len() == 4
                && parts[0] == "projects"
                && parts[2] == kind
                && [parts[1], parts[3]].iter().all(|s| {
                    !s.is_empty()
                        && s.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_.~+%".contains(&b))
                }))
            .then(|| parts[1].to_string())
        }
        let project =
            resource(&self.topic, "topics").context("Use projects/PROJECT/topics/TOPIC")?;
        ensure!(
            resource(&self.subscription, "subscriptions").as_deref() == Some(&project),
            "Use a subscription in the topic's project"
        );
        ensure!(
            self.credentials_file.is_absolute(),
            "Cloud credential path must be absolute"
        );
        let resolved = self.credentials_file.canonicalize()?;
        ensure!(
            !crate::store::in_repository(&resolved),
            "Keep Cloud credentials outside the repository"
        );
        ensure!(
            resolved.metadata()?.permissions().mode() & 0o077 == 0,
            "Cloud credentials must have mode 0600"
        );
        Ok(())
    }
}
pub fn load(dir: &Path) -> Result<Option<Config>> {
    let path = dir.join("push.json");
    if !path.exists() {
        return Ok(None);
    }
    let c: Config = serde_json::from_slice(&std::fs::read(path)?)?;
    if c.enabled {
        c.validate()?;
    }
    Ok(Some(c))
}
pub fn save(dir: &Path, c: &Config) -> Result<()> {
    if c.enabled {
        c.validate()?;
    }
    crate::store::write_private(&dir.join("push.json"), &serde_json::to_vec_pretty(c)?)
}
fn http() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(35))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
fn response(r: reqwest::blocking::Response) -> Result<Value> {
    ensure!(
        r.status().is_success(),
        "Push API returned HTTP {}",
        r.status().as_u16()
    );
    let mut bytes = Vec::new();
    r.take(2_000_001).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 2_000_000, "Push API response is too large");
    if bytes.is_empty() {
        return Ok(json!({}));
    }
    Ok(serde_json::from_slice(&bytes)?)
}
struct Cloud {
    path: PathBuf,
    token: String,
    expires: u64,
    http: reqwest::blocking::Client,
    endpoint: String,
}
impl Cloud {
    fn new(path: PathBuf) -> Result<Self> {
        Ok(Self {
            path,
            token: String::new(),
            expires: 0,
            http: http()?,
            endpoint: "https://pubsub.googleapis.com/v1".into(),
        })
    }
    fn token(&mut self) -> Result<String> {
        let now = auth::now();
        if self.expires > now + 90 {
            return Ok(self.token.clone());
        }
        let c: Value = serde_json::from_slice(&std::fs::read(&self.path)?)?;
        let field = |key: &str| {
            c[key]
                .as_str()
                .with_context(|| format!("Cloud credential missing {key}"))
        };
        let r = match field("type")? {
            "authorized_user" => self
                .http
                .post("https://oauth2.googleapis.com/token")
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("client_id", field("client_id")?),
                    ("client_secret", field("client_secret")?),
                    ("refresh_token", field("refresh_token")?),
                ])
                .send()?,
            "service_account" => {
                let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
                let claims=URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"iss":field("client_email")?,"scope":"https://www.googleapis.com/auth/pubsub","aud":"https://oauth2.googleapis.com/token","iat":now,"exp":now+3600}))?);
                let signing = format!("{header}.{claims}");
                let mut key = tempfile::NamedTempFile::new()?;
                key.write_all(field("private_key")?.as_bytes())?;
                key.flush()?;
                let mut child = Command::new("openssl")
                    .args(["dgst", "-sha256", "-sign"])
                    .arg(key.path())
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()?;
                child.stdin.take().unwrap().write_all(signing.as_bytes())?;
                let output = child.wait_with_output()?;
                ensure!(output.status.success(), "Cloud credential signing failed");
                self.http
                    .post("https://oauth2.googleapis.com/token")
                    .form(&[
                        ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                        (
                            "assertion",
                            &format!("{signing}.{}", URL_SAFE_NO_PAD.encode(output.stdout)),
                        ),
                    ])
                    .send()?
            }
            _ => anyhow::bail!("Use authorized_user or service_account Cloud credentials"),
        };
        let value = response(r)?;
        self.token = value["access_token"]
            .as_str()
            .context("Missing Cloud access token")?
            .into();
        self.expires = now + value["expires_in"].as_u64().unwrap_or(3600);
        Ok(self.token.clone())
    }
    fn call(&mut self, resource: &str, op: &str, body: Value) -> Result<Value> {
        let token = self.token()?;
        response(
            self.http
                .post(format!("{}/{resource}:{op}", self.endpoint))
                .bearer_auth(token)
                .json(&body)
                .send()?,
        )
    }
}
fn matches_account(message: &Value, email: &str) -> bool {
    let Some(data) = message["message"]["data"]
        .as_str()
        .filter(|s| s.len() <= 8192)
    else {
        return false;
    };
    let Ok(bytes) = STANDARD
        .decode(data)
        .or_else(|_| URL_SAFE_NO_PAD.decode(data))
    else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    value["emailAddress"]
        .as_str()
        .is_some_and(|e| e.eq_ignore_ascii_case(email))
        && (value["historyId"].as_u64().is_some()
            || value["historyId"].as_str().is_some_and(|id| {
                !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_digit())
            }))
}
fn healthy(store: &Store) -> Result<()> {
    store.put("push_healthy_at", &auth::now())?;
    store.put(
        "push_status",
        &"Push connected · fallback catch-up every 5 minutes",
    )
}
pub fn interval(store: &Store) -> u64 {
    if store
        .get::<u64>("push_healthy_at")
        .ok()
        .flatten()
        .is_some_and(|t| auth::now().saturating_sub(t) < 90)
    {
        300
    } else {
        30
    }
}
fn cycle(
    dir: &Path,
    store: &Store,
    c: &Config,
    cloud: &mut Cloud,
    commands: &mpsc::Sender<worker::Command>,
) -> Result<()> {
    let mut google = crate::google::Google::open(dir)?;
    let email = store
        .get::<String>("email")?
        .context("Connect Gmail before push setup")?;
    let now = auth::now();
    let watch_topic = store.get::<String>("push_watch_topic")?.unwrap_or_default();
    let renewed = store.get::<u64>("push_watch_renewed")?.unwrap_or(0);
    let expires = store.get::<u64>("push_watch_expires")?.unwrap_or(0);
    if watch_topic != c.topic || now.saturating_sub(renewed) >= 86400 || expires <= now + 86400 {
        // Never use the watch historyId to replace our durable catch-up cursor.
        let expiration = google.watch(&c.topic)?;
        store.put("push_watch_topic", &c.topic)?;
        store.put("push_watch_expires", &expiration)?;
        store.put("push_watch_renewed", &now)?;
    }
    let pulled = cloud.call(&c.subscription, "pull", json!({"maxMessages":100}))?;
    let messages = pulled["receivedMessages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    healthy(store)?;
    if messages.is_empty() {
        std::thread::sleep(Duration::from_millis(500));
        return Ok(());
    }
    process_batch(store, &email, &messages, cloud, &c.subscription, commands)
}
fn process_batch(
    store: &Store,
    email: &str,
    messages: &[Value],
    cloud: &mut Cloud,
    subscription: &str,
    commands: &mpsc::Sender<worker::Command>,
) -> Result<()> {
    let mut discard = Vec::new();
    let mut pending = Vec::new();
    for message in messages {
        if let Some(ack) = message["ackId"].as_str().filter(|s| s.len() <= 4096) {
            if matches_account(message, email) {
                pending.push(ack.to_string())
            } else {
                discard.push(ack.to_string())
            }
        }
    }
    if !discard.is_empty() {
        cloud.call(subscription, "acknowledge", json!({"ackIds":discard}))?;
    }
    if !pending.is_empty() {
        cloud.call(
            subscription,
            "modifyAckDeadline",
            json!({"ackIds":pending,"ackDeadlineSeconds":180}),
        )?;
        let (done, wait) = mpsc::channel();
        commands.send(worker::Command::PushSync(done))?;
        ensure!(
            wait.recv_timeout(Duration::from_secs(120))?,
            "Catch-up failed; push will be redelivered"
        );
        cloud.call(subscription, "acknowledge", json!({"ackIds":pending}))?;
        store.put("push_last_event", &auth::now())?;
    }
    Ok(())
}
pub fn start(dir: PathBuf, commands: mpsc::Sender<worker::Command>) {
    std::thread::spawn(move || {
        let Ok(store) = Store::open(Some(&dir)) else {
            return;
        };
        let mut cloud: Option<Cloud> = None;
        let mut was_enabled = false;
        loop {
            let result = (|| -> Result<bool> {
                let Some(c) = load(&dir)?.filter(|c| c.enabled) else {
                    if store
                        .get::<String>("push_watch_topic")?
                        .is_some_and(|s| !s.is_empty())
                    {
                        crate::google::Google::open(&dir)?.stop_watch()?;
                        store.put("push_watch_topic", &"")?;
                    }
                    store.put("push_healthy_at", &0u64)?;
                    store.put("push_status", &"Polling every 30 seconds · push disabled")?;
                    return Ok(false);
                };
                if cloud
                    .as_ref()
                    .is_none_or(|cloud| cloud.path != c.credentials_file)
                {
                    cloud = Some(Cloud::new(c.credentials_file.clone())?)
                }
                cycle(&dir, &store, &c, cloud.as_mut().unwrap(), &commands)?;
                Ok(true)
            })();
            match result {
                Ok(true) => {
                    was_enabled = true;
                }
                Ok(false) => {
                    if was_enabled {
                        let _ = commands.send(worker::Command::Sync);
                    }
                    was_enabled = false;
                    std::thread::sleep(Duration::from_secs(5));
                }
                Err(_) => {
                    cloud = None;
                    let _ = store.put("push_healthy_at", &0u64);
                    let _=store.put("push_status",&"Push unavailable · polling every 30 seconds; check Cloud credentials and setup");
                    // Wake the fallback immediately instead of retaining a five-minute deadline.
                    let _ = commands.send(worker::Command::Sync);
                    std::thread::sleep(Duration::from_secs(30));
                }
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_matching_account_notifications_wake_sync() {
        let notification = |email: &str, id: &str| json!({"message":{"data":STANDARD.encode(serde_json::to_vec(&json!({"emailAddress":email,"historyId":id})).unwrap())}});
        assert!(matches_account(
            &notification("ANY@example.test", "42"),
            "any@example.test"
        ));
        assert!(!matches_account(
            &notification("other@example.test", "42"),
            "any@example.test"
        ));
        assert!(!matches_account(
            &notification("any@example.test", "not-a-cursor"),
            "any@example.test"
        ));
        assert!(!matches_account(
            &json!({"message":{"data":"not-base64"}}),
            "any@example.test"
        ));
    }
    #[test]
    fn gmail_numeric_history_ids_are_valid_wakeups() {
        let message = json!({"message":{"data":URL_SAFE_NO_PAD.encode(br#"{"emailAddress":"any@example.test","historyId":9876543210}"#)}});
        assert!(matches_account(&message, "any@example.test"));
    }
    #[test]
    fn fallback_when_push_unhealthy_or_stale() {
        let store = Store::open(None).unwrap();
        assert_eq!(interval(&store), 30);
        healthy(&store).unwrap();
        assert_eq!(interval(&store), 300);
        store
            .put("push_healthy_at", &auth::now().saturating_sub(91))
            .unwrap();
        assert_eq!(interval(&store), 30);
    }
    fn mock_cloud(count: usize) -> (Cloud, std::thread::JoinHandle<Vec<(String, Value)>>) {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while requests.len() < count && std::time::Instant::now() < deadline {
                let Ok((mut socket, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut byte = [0; 1];
                while !bytes.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                }
                let header = String::from_utf8(bytes).unwrap();
                let length: usize = header
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|s| s.trim().parse().unwrap())
                    })
                    .unwrap();
                let mut body = vec![0; length];
                socket.read_exact(&mut body).unwrap();
                requests.push((
                    header
                        .lines()
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .nth(1)
                        .unwrap()
                        .into(),
                    serde_json::from_slice(&body).unwrap(),
                ));
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
            }
            requests
        });
        (
            Cloud {
                path: PathBuf::new(),
                token: "synthetic".into(),
                expires: auth::now() + 3600,
                http: http().unwrap(),
                endpoint,
            },
            server,
        )
    }
    #[test]
    fn coalesces_push_and_only_acknowledges_after_successful_catchup() {
        let (mut cloud, server) = mock_cloud(3);
        let message = |ack: &str, email: &str| json!({"ackId":ack,"message":{"data":STANDARD.encode(serde_json::to_vec(&json!({"emailAddress":email,"historyId":42})).unwrap())}});
        let messages = vec![
            message("first", "any@example.test"),
            message("duplicate", "any@example.test"),
            message("foreign", "other@example.test"),
        ];
        let store = Store::open(None).unwrap();
        store.put("history", &"21").unwrap();
        let (commands, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let worker::Command::PushSync(done) = rx.recv_timeout(Duration::from_secs(3)).unwrap()
            else {
                panic!("Expected catch-up")
            };
            done.send(true).unwrap();
            assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        });
        process_batch(
            &store,
            "any@example.test",
            &messages,
            &mut cloud,
            "projects/p/subscriptions/s",
            &commands,
        )
        .unwrap();
        worker.join().unwrap();
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[0].0.ends_with(":acknowledge"));
        assert_eq!(requests[0].1["ackIds"], json!(["foreign"]));
        assert!(requests[1].0.ends_with(":modifyAckDeadline"));
        assert!(requests[2].0.ends_with(":acknowledge"));
        assert_eq!(requests[2].1["ackIds"], json!(["first", "duplicate"]));
        assert_eq!(store.get::<String>("history").unwrap().unwrap(), "21");
        assert!(store.get::<u64>("push_last_event").unwrap().is_some());
    }
    #[test]
    fn failed_catchup_keeps_push_unacknowledged() {
        let (mut cloud, server) = mock_cloud(1);
        let message = json!({"ackId":"retry","message":{"data":STANDARD.encode(br#"{"emailAddress":"any@example.test","historyId":42}"#)}});
        let store = Store::open(None).unwrap();
        let (commands, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            if let worker::Command::PushSync(done) = rx.recv().unwrap() {
                done.send(false).unwrap();
            }
        });
        assert!(
            process_batch(
                &store,
                "any@example.test",
                &[message],
                &mut cloud,
                "projects/p/subscriptions/s",
                &commands
            )
            .is_err()
        );
        worker.join().unwrap();
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].0.ends_with(":modifyAckDeadline"));
        assert!(store.get::<u64>("push_last_event").unwrap().is_none());
    }
}
