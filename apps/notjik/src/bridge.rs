//! Single-owner TDLib event loop. Requests are asynchronous so downloads, reads,
//! and authentication never stall notification delivery or socket acceptance.
use crate::{
    native::Td,
    telegram::{identifier, socket_path},
    telegram_text::{describe, sticker, text, transcription, trunc},
    util::*,
};
use anyhow::{ensure, Context, Result};
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const MAX_FILE: u64 = 20 * 1024 * 1024;
type Callback = Box<dyn FnOnce(&mut Bridge, Value)>;
type Answer = mpsc::SyncSender<Value>;
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
pub fn key(chat: i64, message: i64) -> String {
    format!("{}{chat}/{message}", if chat < 0 { "chat" } else { "" })
}
fn notice_id(k: &str) -> String {
    fn enc(s: &str) -> String {
        s.as_bytes()
            .iter()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"~()*!.'-_".contains(b) {
                    (*b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    }
    format!("named-{}/{}", enc("Telegram via Notjik"), enc(k))
}
fn checked(answer: Answer) -> Callback {
    Box::new(move |_, v| {
        let _ = answer.send(if v["@type"] == "error" {
            json!({"ok":false,"error":v["message"]})
        } else {
            json!({"ok":true})
        });
    })
}
#[derive(Clone)]
struct Entry {
    message: Value,
    notice: Value,
    preview: bool,
    published: bool,
    file: i64,
    avatar: i64,
    generation: u64,
}
struct MediaJob {
    key: String,
    generation: u64,
    emoji: Option<String>,
    format: String,
    result: mpsc::Receiver<String>,
}
pub struct Bridge {
    td: Td,
    root: PathBuf,
    sequence: u64,
    pending: HashMap<String, (Instant, Callback)>,
    entries: HashMap<String, Entry>,
    order: VecDeque<String>,
    notifications: HashMap<(i64, i64), String>,
    chats: HashMap<i64, Value>,
    downloads: HashMap<i64, HashSet<String>>,
    avatars: HashMap<i64, HashSet<String>>,
    emojis: HashMap<i64, HashSet<(String, String)>>,
    jobs: Vec<MediaJob>,
    started: f64,
    delivery: Value,
    generation: u64,
    voice_uploads: HashMap<i64, Value>,
    voice_sends: HashMap<(i64, i64), String>,
    ready: bool,
    closed: bool,
    connected: bool,
    auth_state: Value,
    auth_error: String,
    creds: Value,
    login: bool,
    options: HashSet<String>,
    online: Option<bool>,
    presence_pending: Option<bool>,
    presence_generation: u64,
}
impl Bridge {
    fn new(root: &Path, login: bool) -> Result<Self> {
        let path = telegram_dir().join("credentials.json");
        private_dir(&telegram_dir())?;
        let creds = if path.exists() {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            read_json(&path)?
        } else {
            ensure!(
                login,
                "Telegram is not configured. Run notjikctl telegram login in a terminal."
            );
            println!("Create an API application at https://my.telegram.org/apps. Credentials stay on this computer.");
            let id = prompt("API ID: ", false)?.parse::<i64>()?;
            let hash = prompt("API hash: ", true)?;
            ensure!(
                id > 0 && regex::Regex::new(r"^[0-9a-fA-F]{32}$")?.is_match(&hash),
                "Invalid API ID or hash"
            );
            let mut key = [0; 32];
            fs::File::open("/dev/urandom")?.read_exact(&mut key)?;
            let v = json!({"api_id":id,"api_hash":hash,"key":base64::engine::general_purpose::STANDARD.encode(key)});
            private_write(&path, v.to_string().as_bytes())?;
            v
        };
        private_dir(&telegram_dir().join("rendered"))?;
        Ok(Self::with_td(root, login, creds, Td::new(root)?))
    }
    fn with_td(root: &Path, login: bool, creds: Value, td: Td) -> Self {
        Self {
            td,
            root: root.into(),
            sequence: 0,
            pending: HashMap::new(),
            entries: HashMap::new(),
            order: VecDeque::new(),
            notifications: HashMap::new(),
            chats: HashMap::new(),
            downloads: HashMap::new(),
            avatars: HashMap::new(),
            emojis: HashMap::new(),
            jobs: vec![],
            started: now(),
            delivery: json!({"notificationEvents":0,"lastNotificationAt":0,"ignoredType":0,"ignoredOutgoing":0,"ignoredOld":0,"publishAttempts":0,"publishFailures":0}),
            generation: 0,
            voice_uploads: HashMap::new(),
            voice_sends: HashMap::new(),
            ready: false,
            closed: false,
            connected: false,
            auth_state: Value::Null,
            auth_error: String::new(),
            creds,
            login,
            options: HashSet::new(),
            online: None,
            presence_pending: None,
            presence_generation: 0,
        }
    }
    fn send(&mut self, method: &str, mut fields: Value, callback: Option<Callback>) {
        self.sequence += 1;
        let extra = self.sequence.to_string();
        fields["@type"] = json!(method);
        fields["@extra"] = json!(extra);
        if let Err(e) = self.td.send(&fields) {
            if let Some(cb) = callback {
                cb(self, json!({"@type":"error","message":e.to_string()}));
            }
        } else if let Some(cb) = callback {
            self.pending
                .insert(extra, (Instant::now() + Duration::from_secs(30), cb));
        }
    }
    fn receive(&mut self) -> Result<Option<Value>> {
        let raw = self.td.receive(0.05)?;
        let expired = self
            .pending
            .iter()
            .filter(|(_, v)| v.0 < Instant::now())
            .map(|(k, _)| k.clone())
            .collect::<Vec<_>>();
        for key in expired {
            if let Some((_, cb)) = self.pending.remove(&key) {
                cb(
                    self,
                    json!({"@type":"error","message":"Telegram request timed out"}),
                );
            }
        }
        if let Some(v) = raw {
            if let Some((_, cb)) = self.pending.remove(text(&v["@extra"])) {
                cb(self, v);
                Ok(None)
            } else {
                Ok(Some(v))
            }
        } else {
            Ok(None)
        }
    }
    fn count(&mut self, key: &str) {
        self.delivery[key] = json!(self.delivery[key].as_u64().unwrap_or(0) + 1);
    }
    fn show(&mut self, key: &str) {
        if let Some(e) = self.entries.get(key) {
            let notice = e.notice.clone();
            self.count("publishAttempts");
            let ok = ipc(&self.root, &json!({"op":"send","notice":notice})).is_ok();
            if !ok {
                self.count("publishFailures");
            }
            if let Some(e) = self.entries.get_mut(key) {
                e.published = ok;
            }
        }
    }
    fn remove(&mut self, key: &str) {
        if self.entries.remove(key).is_none() {
            return;
        }
        self.order.retain(|k| k != key);
        let mut cancel = vec![];
        self.downloads.retain(|id, keys| {
            keys.remove(key);
            if keys.is_empty() {
                cancel.push(*id);
                false
            } else {
                true
            }
        });
        for id in cancel {
            self.send(
                "cancelDownloadFile",
                json!({"file_id":id,"only_if_pending":false}),
                None,
            );
        }
        self.avatars.retain(|_, keys| {
            keys.remove(key);
            !keys.is_empty()
        });
        self.emojis.retain(|_, targets| {
            targets.retain(|(k, _)| k != key);
            !targets.is_empty()
        });
        self.notifications.retain(|_, k| k != key);
        let _ = ipc(
            &self.root,
            &json!({"op":"telegramRemove","id":notice_id(key)}),
        );
    }
    fn chat_result(&mut self, chat: i64, v: Value) {
        if v["@type"] != "chat" {
            return;
        }
        self.chats.insert(chat, v.clone());
        let keys = self
            .order
            .iter()
            .filter(|k| self.entries[*k].message["chat_id"] == chat)
            .cloned()
            .collect::<Vec<_>>();
        for k in keys {
            self.entries.get_mut(&k).unwrap().notice["title"] =
                json!(trunc(v["title"].as_str().unwrap_or("Telegram"), 4096));
            self.avatar(&k);
            self.show(&k);
        }
    }
    fn avatar(&mut self, key: &str) {
        let Some(e) = self.entries.get_mut(key) else {
            return;
        };
        let chat = e.message["chat_id"].as_i64().unwrap();
        let photo = &self.chats.get(&chat).unwrap_or(&Value::Null)["photo"];
        let file = photo
            .get("big")
            .filter(|v| v.is_object())
            .or_else(|| photo.get("small"))
            .cloned()
            .unwrap_or(Value::Null);
        let id = file["id"].as_i64().unwrap_or(0);
        e.avatar = id;
        e.notice.as_object_mut().unwrap().remove("image");
        self.avatars.retain(|_, keys| {
            keys.remove(key);
            !keys.is_empty()
        });
        if id == 0 || file_size(&file) > MAX_FILE {
            return;
        }
        let pending = self.avatars.contains_key(&id);
        self.avatars.entry(id).or_default().insert(key.into());
        if file["local"]["is_downloading_completed"] == true {
            self.avatar_result(id, file);
        } else if !pending {
            self.send(
                "downloadFile",
                download(id, 8),
                Some(Box::new(move |b, v| b.avatar_result(id, v))),
            );
        }
    }
    fn local_media(&self, file: &Value, avatar: bool) -> Result<PathBuf> {
        let p = PathBuf::from(text(&file["local"]["path"])).canonicalize()?;
        let root = telegram_dir().canonicalize()?;
        ensure!(
            [
                "files",
                if avatar {
                    "db/profile_photos"
                } else {
                    "db/stickers"
                }
            ]
            .iter()
            .any(|part| p.starts_with(root.join(part))),
            "Unexpected media path"
        );
        let m = fs::metadata(&p)?;
        ensure!(m.is_file() && m.len() <= MAX_FILE, "Invalid media file");
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600))?;
        Ok(p)
    }
    fn avatar_result(&mut self, id: i64, file: Value) {
        if !self.avatars.contains_key(&id) {
            return;
        }
        if file["@type"] == "error" || file_size(&file) > MAX_FILE {
            self.avatars.remove(&id);
            if file_size(&file) > MAX_FILE {
                self.send(
                    "cancelDownloadFile",
                    json!({"file_id":id,"only_if_pending":false}),
                    None,
                );
            }
            return;
        }
        if file["local"]["is_downloading_completed"] != true {
            return;
        }
        let keys = self.avatars.remove(&id).unwrap();
        let Ok(p) = self.local_media(&file, true) else {
            return;
        };
        let Ok(source) = uri(&p) else { return };
        for k in keys {
            if let Some(e) = self.entries.get_mut(&k).filter(|e| e.avatar == id) {
                e.notice["image"] = json!(source);
                let chat = e.message["chat_id"].as_i64().unwrap();
                if let Some(c) = self.chats.get_mut(&chat) {
                    for size in ["big", "small"] {
                        if c["photo"][size]["id"] == id {
                            merge(&mut c["photo"][size], &file);
                        }
                    }
                }
                self.show(&k);
            }
        }
    }
    fn add(&mut self, group: i64, chat: i64, n: Value) {
        self.count("notificationEvents");
        self.delivery["lastNotificationAt"] = json!(now());
        let item = &n["type"];
        if item["@type"] != "notificationTypeNewMessage" {
            self.count("ignoredType");
            return;
        }
        let mut message = item["message"].clone();
        if message["is_outgoing"] == true {
            self.count("ignoredOutgoing");
            return;
        }
        if n["date"]
            .as_f64()
            .or_else(|| message["date"].as_f64())
            .unwrap_or(0.)
            < self.started - 30.
        {
            self.count("ignoredOld");
            return;
        }
        if identifier(&json!(chat)).is_err() {
            return;
        }
        let Ok(mid) = identifier(&message["id"]) else {
            return;
        };
        message["chat_id"] = json!(chat);
        let key = key(chat, mid);
        self.notifications
            .insert((group, n["id"].as_i64().unwrap_or(0)), key.clone());
        let preview = item["show_preview"] == true;
        let (body, mut data, file) = describe(&message, preview);
        data["chatId"] = json!(chat.to_string());
        data["messageId"] = json!(mid.to_string());
        let title = self
            .chats
            .get(&chat)
            .and_then(|c| c["title"].as_str())
            .unwrap_or("Telegram");
        let timeout =
            if data["kind"] != "text" || data["parts"].as_array().is_some_and(|a| !a.is_empty()) {
                0
            } else {
                12000
            };
        let notice = json!({"id":key,"app":"Telegram via Notjik","title":trunc(title,4096),"body":trunc(&body,65536),"type":"telegram","group":chat.to_string(),"icon":"org.telegram.desktop","timeout":timeout,"resident":true,"data":data});
        self.generation += 1;
        let fid = file["id"].as_i64().unwrap_or(0);
        if !self.entries.contains_key(&key) {
            self.order.push_back(key.clone());
        }
        self.entries.insert(
            key.clone(),
            Entry {
                message,
                notice,
                preview,
                published: false,
                file: fid,
                avatar: 0,
                generation: self.generation,
            },
        );
        while self.order.len() > 100 {
            let k = self.order.front().unwrap().clone();
            self.remove(&k);
        }
        self.avatar(&key);
        self.show(&key);
        self.load_emojis(&key);
        if !self.chats.contains_key(&chat) {
            self.send(
                "getChat",
                json!({"chat_id":chat}),
                Some(Box::new(move |b, v| b.chat_result(chat, v))),
            );
        }
        if fid != 0 {
            if file_size(&file) > MAX_FILE {
                self.status(&key, "Attachment exceeds 20 MiB · open in Telegram");
                return;
            }
            self.downloads.entry(fid).or_default().insert(key.clone());
            if file["local"]["is_downloading_completed"] == true {
                self.file_update(file);
            } else {
                self.send(
                    "downloadFile",
                    download(fid, 16),
                    Some(Box::new(move |b, v| b.download_result(fid, v))),
                );
            }
        } else if data["kind"] != "text" {
            self.status(&key, "Attachment unavailable · open in Telegram");
        }
    }
    fn status(&mut self, key: &str, status: &str) {
        if let Some(e) = self.entries.get_mut(key) {
            e.notice["data"]["status"] = json!(status);
            self.show(key);
        }
    }
    fn load_emojis(&mut self, key: &str) {
        let Some(e) = self.entries.get(key) else {
            return;
        };
        let ids = e.notice["data"]["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["kind"] == "emoji")
            .map(|p| text(&p["emojiId"]).to_string())
            .collect::<HashSet<_>>();
        if ids.is_empty() {
            return;
        }
        let generation = e.generation;
        let key = key.to_string();
        self.send("getCustomEmojiStickers",json!({"custom_emoji_ids":ids.iter().filter_map(|s|s.parse::<i64>().ok()).collect::<Vec<_>>()}),Some(Box::new(move|b,v|{if !b.entries.get(&key).is_some_and(|e|e.generation==generation){return}for s in v["stickers"].as_array().into_iter().flatten(){let id=s["full_type"]["custom_emoji_id"].as_str().map(str::to_string).unwrap_or_else(||s["full_type"]["custom_emoji_id"].to_string());if !ids.contains(&id){continue}let(media,file)=sticker(s,"sticker");let fid=file["id"].as_i64().unwrap_or(0);if fid==0||file_size(&file)>MAX_FILE{continue}if let Some(parts)=b.entries.get_mut(&key).unwrap().notice["data"]["parts"].as_array_mut(){for p in parts{if p["emojiId"]==id{p["format"]=media["format"].clone();p["fileId"]=json!(fid);}}}let pending=b.emojis.contains_key(&fid);b.emojis.entry(fid).or_default().insert((key.clone(),id));if file["local"]["is_downloading_completed"]==true{b.emoji_update(file);}else if !pending{b.send("downloadFile",download(fid,8),Some(Box::new(move|b,mut v|{v["id"]=json!(fid);b.emoji_update(v);})));}}})));
    }
    fn emoji_update(&mut self, file: Value) {
        let id = file["id"].as_i64().unwrap_or(0);
        if !self.emojis.contains_key(&id) {
            return;
        }
        if file["@type"] == "error" || file_size(&file) > MAX_FILE {
            self.emojis.remove(&id);
            return;
        }
        if file["local"]["is_downloading_completed"] != true {
            return;
        }
        let targets = self.emojis.remove(&id).unwrap();
        let Ok(path) = self.local_media(&file, false) else {
            return;
        };
        for (key, emoji) in targets {
            let format = self
                .entries
                .get(&key)
                .and_then(|e| e.notice["data"]["parts"].as_array())
                .and_then(|parts| {
                    parts
                        .iter()
                        .find(|p| p["emojiId"] == emoji && p["fileId"] == id)
                })
                .map(|p| text(&p["format"]).to_string());
            if let Some(format) = format {
                self.prepare_media(&key, &path, &format, Some(emoji));
            }
        }
    }
    fn media_ready(
        &mut self,
        key: &str,
        generation: u64,
        emoji: Option<&str>,
        source: &str,
        format: &str,
    ) {
        if let Some(e) = self
            .entries
            .get_mut(key)
            .filter(|e| e.generation == generation)
        {
            let data = &mut e.notice["data"];
            if let Some(id) = emoji {
                if let Some(parts) = data["parts"].as_array_mut() {
                    for p in parts {
                        if p["emojiId"] == id {
                            p["source"] = json!(source);
                            p["format"] = json!(format);
                        }
                    }
                }
            } else {
                merge(
                    data,
                    &json!({"source":source,"format":format,"status":if source.is_empty(){"Animation unavailable · open in Telegram"}else{""}}),
                );
            }
            self.show(key);
        }
    }
    fn prepare_media(&mut self, key: &str, path: &Path, format: &str, emoji: Option<String>) {
        let Some(e) = self.entries.get(key) else {
            return;
        };
        let generation = e.generation;
        if !["tgs", "webm", "webp"].contains(&format) {
            self.media_ready(
                key,
                generation,
                emoji.as_deref(),
                &uri(path).unwrap_or_default(),
                format,
            );
            return;
        }
        let stamp = fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let digest = format!(
            "{:x}",
            Sha256::digest(format!("{}{stamp}v1", path.display()).as_bytes())
        );
        let out_format = if format == "webp" { "png" } else { "gif" };
        let output = telegram_dir()
            .join("rendered")
            .join(format!("{digest}.{out_format}"));
        if output.is_file() {
            self.media_ready(
                key,
                generation,
                emoji.as_deref(),
                &uri(&output).unwrap_or_default(),
                out_format,
            );
            return;
        }
        if self.jobs.len() >= 32 {
            self.media_ready(key, generation, emoji.as_deref(), "", out_format);
            return;
        }
        let (tx, rx) = mpsc::channel();
        let source = path.to_path_buf();
        let format = format.to_string();
        std::thread::spawn(move || {
            let result = (|| -> Result<String> {
                // At most two conversions run concurrently; queued jobs remain bounded.
                static CONVERTERS: std::sync::Mutex<usize> = std::sync::Mutex::new(0);
                loop {
                    let mut n = CONVERTERS.lock().unwrap();
                    if *n < 2 {
                        *n += 1;
                        break;
                    }
                    drop(n);
                    std::thread::sleep(Duration::from_millis(50));
                }
                struct Slot;
                impl Drop for Slot {
                    fn drop(&mut self) {
                        *CONVERTERS.lock().unwrap() -= 1;
                    }
                }
                let _slot = Slot;
                let pending = tempfile::Builder::new()
                    .suffix(".tmp")
                    .tempfile_in(output.parent().unwrap())?;
                run(
                    Command::new(std::env::current_exe()?)
                        .arg("media-render")
                        .arg(source)
                        .arg(pending.path())
                        .arg(format),
                    Duration::from_secs(35),
                )?;
                pending.persist(&output)?;
                Ok(uri(&output)?)
            })();
            let _ = tx.send(result.unwrap_or_default());
        });
        self.jobs.push(MediaJob {
            key: key.into(),
            generation,
            emoji,
            format: out_format.into(),
            result: rx,
        });
    }
    fn poll_media(&mut self) {
        let mut i = 0;
        while i < self.jobs.len() {
            match self.jobs[i].result.try_recv() {
                Ok(source) => {
                    let job = self.jobs.remove(i);
                    self.media_ready(
                        &job.key,
                        job.generation,
                        job.emoji.as_deref(),
                        &source,
                        &job.format,
                    )
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    let job = self.jobs.remove(i);
                    self.media_ready(
                        &job.key,
                        job.generation,
                        job.emoji.as_deref(),
                        "",
                        &job.format,
                    )
                }
                Err(mpsc::TryRecvError::Empty) => i += 1,
            }
        }
    }
    fn download_result(&mut self, id: i64, file: Value) {
        if file["@type"] == "error" {
            for key in self.downloads.remove(&id).unwrap_or_default() {
                self.status(&key, "Download failed · open in Telegram");
            }
        } else if file["@type"] == "file" {
            self.file_update(file);
        }
    }
    fn file_update(&mut self, file: Value) {
        let id = file["id"].as_i64().unwrap_or(0);
        if !self.downloads.contains_key(&id) {
            return;
        }
        if file_size(&file) > MAX_FILE {
            self.send(
                "cancelDownloadFile",
                json!({"file_id":id,"only_if_pending":false}),
                None,
            );
            self.download_result(id, json!({"@type":"error"}));
            return;
        }
        if file["local"]["is_downloading_completed"] != true {
            return;
        }
        let keys = self.downloads.remove(&id).unwrap();
        let path = self.local_media(&file, false).ok();
        for key in keys {
            if let Some(e) = self.entries.get_mut(&key).filter(|e| e.file == id) {
                let d = &mut e.notice["data"];
                if path.is_some() && ["sticker", "emoji", "animation"].contains(&text(&d["kind"])) {
                    d["status"] = json!("Rendering animation…");
                    let format = text(&d["format"]).to_string();
                    self.prepare_media(&key, path.as_ref().unwrap(), &format, None);
                } else {
                    d["source"] =
                        json!(path.as_ref().and_then(|p| uri(p).ok()).unwrap_or_default());
                    d["status"] = json!(if path.is_some() {
                        ""
                    } else {
                        "Attachment unavailable · open in Telegram"
                    });
                }
                self.show(&key);
            }
        }
    }
    fn handle(&mut self, v: Value) {
        match text(&v["@type"]) {
            "updateNewChat" => {
                if let Some(id) = v["chat"]["id"].as_i64() {
                    self.chat_result(id, v["chat"].clone());
                }
            }
            "updateChatTitle" | "updateChatPhoto" => {
                let id = v["chat_id"].as_i64().unwrap_or(0);
                if let Some(mut chat) = self.chats.get(&id).cloned() {
                    let field = if v["@type"] == "updateChatTitle" {
                        "title"
                    } else {
                        "photo"
                    };
                    chat[field] = v[field].clone();
                    self.chat_result(id, chat);
                }
            }
            "updateNotificationGroup" => {
                let group = v["notification_group_id"].as_i64().unwrap_or(0);
                for id in v["removed_notification_ids"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    if let Some(key) = self
                        .notifications
                        .remove(&(group, id.as_i64().unwrap_or(0)))
                    {
                        if !self.notifications.values().any(|k| k == &key) {
                            self.remove(&key);
                        }
                    }
                }
                for n in v["added_notifications"].as_array().into_iter().flatten() {
                    self.add(group, v["chat_id"].as_i64().unwrap_or(0), n.clone());
                }
            }
            "updateNotification" => {
                let n = &v["notification"];
                if let Some(chat) = n["type"]["message"]["chat_id"].as_i64() {
                    self.add(
                        v["notification_group_id"].as_i64().unwrap_or(0),
                        chat,
                        n.clone(),
                    );
                }
            }
            "updateFile" => {
                let f = &v["file"];
                self.emoji_update(f.clone());
                self.avatar_result(f["id"].as_i64().unwrap_or(0), f.clone());
                self.file_update(f.clone());
                self.voice_file(f);
            }
            "updateMessageSendSucceeded" | "updateMessageSendFailed" => {
                let chat = v["message"]["chat_id"].as_i64().unwrap_or(0);
                let old = v["old_message_id"].as_i64().unwrap_or(0);
                let success = v["@type"] == "updateMessageSendSucceeded";
                if let Some(key) = self.voice_sends.remove(&(chat, old)) {
                    self.voice_status(
                        &key,
                        if success {
                            "Voice reply sent"
                        } else {
                            "Voice reply failed · open Telegram to retry"
                        },
                    );
                }
                if success {
                    let ids = self
                        .voice_uploads
                        .iter()
                        .filter(|(_, u)| u["chat"] == chat && u["message"] == old)
                        .map(|(id, _)| *id)
                        .collect::<Vec<_>>();
                    for id in ids {
                        self.finish_upload(id);
                    }
                }
            }
            "updateDeleteMessages" if v["from_cache"] != true => {
                for id in v["message_ids"].as_array().into_iter().flatten() {
                    self.remove(&key(
                        v["chat_id"].as_i64().unwrap_or(0),
                        id.as_i64().unwrap_or(0),
                    ));
                }
            }
            "updateMessageContent" => {
                let chat = v["chat_id"].as_i64().unwrap_or(0);
                let k = key(chat, v["message_id"].as_i64().unwrap_or(0));
                if let Some(e) = self.entries.get_mut(&k) {
                    let old = &e.message["content"];
                    let new = &v["new_content"];
                    if old["@type"] == "messageVoiceNote" && new["@type"] == "messageVoiceNote" {
                        let mut before = old["voice_note"].clone();
                        let mut after = new["voice_note"].clone();
                        if let Some(m) = before.as_object_mut() {
                            m.remove("speech_recognition_result");
                        }
                        if let Some(m) = after.as_object_mut() {
                            m.remove("speech_recognition_result");
                        }
                        if before == after {
                            e.message["content"] = new.clone();
                            if e.notice["data"]["kind"] == "voice" {
                                e.notice["data"]
                                    .as_object_mut()
                                    .unwrap()
                                    .remove("transcriptionError");
                                merge(
                                    &mut e.notice["data"],
                                    &transcription(&new["voice_note"]["speech_recognition_result"]),
                                );
                                self.show(&k);
                            }
                            return;
                        }
                    }
                    let mut message = e.message.clone();
                    message["content"] = new.clone();
                    let preview = e.preview;
                    let reference = self
                        .notifications
                        .iter()
                        .find(|(_, v)| *v == &k)
                        .map(|(r, _)| *r);
                    if let Some((group, id)) = reference {
                        self.add(group,chat,json!({"id":id,"date":now(),"type":{"@type":"notificationTypeNewMessage","show_preview":preview,"message":message}}));
                    }
                }
            }
            _ => {}
        }
    }
    fn voice_status(&mut self, key: &str, status: &str) {
        if let Some(e) = self.entries.get_mut(key) {
            e.notice["data"]["voiceReplyStatus"] = json!(status);
            self.show(key);
        }
    }
    fn finish_upload(&mut self, id: i64) {
        if let Some(u) = self.voice_uploads.remove(&id) {
            let p = PathBuf::from(text(&u["path"]));
            let _ = fs::remove_file(&p);
            let _ = fs::remove_file(p.with_extension("json"));
        }
    }
    fn voice_file(&mut self, file: &Value) {
        if file["remote"]["is_uploading_completed"] == true {
            self.finish_upload(file["id"].as_i64().unwrap_or(0));
        }
    }
    fn recover_uploads(&mut self) {
        let directory = telegram_dir().join("voice-outbox");
        let Ok(files) = fs::read_dir(&directory) else {
            return;
        };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().is_none_or(|s| s != "json")
                || !p
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("voice-")
            {
                continue;
            }
            if let Ok(mut v) = read_json(&p) {
                let path = p.with_extension("ogg");
                if !path.is_file() {
                    let _ = fs::remove_file(p);
                    continue;
                }
                if let Some(id) = v["file_id"].as_i64() {
                    v["path"] = json!(path);
                    self.voice_sends.insert(
                        (
                            v["chat"].as_i64().unwrap_or(0),
                            v["message"].as_i64().unwrap_or(0),
                        ),
                        text(&v["origin"]).to_string(),
                    );
                    self.voice_uploads.insert(id, v);
                    self.send(
                        "getFile",
                        json!({"file_id":id}),
                        Some(Box::new(|b, v| b.voice_file(&v))),
                    );
                }
            }
        }
    }
    fn voice_reply(
        &mut self,
        request: &Value,
        message: &Value,
        key: String,
        answer: Answer,
    ) -> Result<()> {
        let (path, duration) = crate::recording::copy_for_upload(
            &request["token"],
            &telegram_dir().join("voice-outbox"),
        )?;
        let chat = message["chat_id"].as_i64().unwrap();
        let mid = message["id"].as_i64();
        self.send("sendMessage",json!({"chat_id":chat,"topic_id":message["topic_id"],"reply_to":mid.map(|id|json!({"@type":"inputMessageReplyToMessage","message_id":id})),"input_message_content":{"@type":"inputMessageVoiceNote","voice_note":{"@type":"inputVoiceNote","voice_note":{"@type":"inputFileLocal","path":path},"duration":duration.ceil().min(300.),"waveform":""}}}),Some(Box::new(move|b,v|{if v["@type"]!="message"{let uncertain=text(&v["message"]).to_lowercase().contains("timed out");if !uncertain{let _=fs::remove_file(&path);}let _=answer.send(json!({"ok":false,"uncertain":uncertain,"error":if uncertain{"Telegram did not confirm; check the chat before retrying"}else{v["message"].as_str().unwrap_or("Telegram rejected the voice reply")}}));return}let mid=v["id"].as_i64().unwrap_or(0);b.voice_sends.insert((chat,mid),key.clone());let file=&v["content"]["voice_note"]["voice"];if let Some(id)=file["id"].as_i64().filter(|id|*id!=0){let upload=json!({"file_id":id,"path":path,"chat":chat,"message":mid,"origin":key});b.voice_uploads.insert(id,upload.clone());if private_write(&path.with_extension("json"),upload.to_string().as_bytes()).is_err(){let _=answer.send(json!({"ok":false,"uncertain":true,"error":"Reply accepted but upload recovery could not be saved; check Telegram before retrying"}));return}b.voice_file(file);}b.voice_status(&key,"Voice reply queued");let _=answer.send(json!({"ok":true}));})));
        Ok(())
    }
    fn read(&mut self, chat: i64, mid: i64, answer: Answer) {
        self.send("viewMessages",json!({"chat_id":chat,"message_ids":[mid],"source":{"@type":"messageSourceNotification"},"force_read":true}),Some(checked(answer)));
    }
    fn safe_boundary(&mut self, message: Value, through: bool, answer: Answer) {
        let chat = message["chat_id"].as_i64().unwrap();
        let mid = message["id"].as_i64().unwrap();
        if through {
            self.read(chat, mid, answer);
            return;
        }
        if !message["topic_id"].is_null() && message["topic_id"] != 0
            || message["message_thread_id"].as_i64().unwrap_or(0) != 0
        {
            let _ = answer.send(json!({"ok":true,"skipped":"Cannot verify a topic read boundary"}));
            return;
        }
        self.send("getChat",json!({"chat_id":chat}),Some(Box::new(move|b,v|{if v["@type"]!="chat"||v["id"]!=chat{let _=answer.send(json!({"ok":true,"skipped":"Cannot verify the chat read boundary"}));return}let boundary=v["last_read_inbox_message_id"].as_i64().unwrap_or(0);if boundary>=mid{let _=answer.send(json!({"ok":true,"skipped":"Message already read"}));}else if v["unread_count"]==1&&v["last_message"]["id"]==mid{b.read(chat,mid,answer);}else{b.send("getChatHistory",json!({"chat_id":chat,"from_message_id":mid,"offset":0,"limit":2,"only_local":false}),Some(Box::new(move|b,v|{let list=v["messages"].as_array();if v["@type"]=="messages"&&list.is_some_and(|a|a.len()>=2&&a[0]["id"]==mid&&a[..2].iter().all(|m|m["chat_id"]==chat)&&a[1]["id"].as_i64().is_some_and(|id|id>0&&id<=boundary)){b.read(chat,mid,answer);}else{let _=answer.send(json!({"ok":true,"skipped":"Earlier unread messages may be affected"}));}})));}})));
    }
    fn resolve_read(
        &mut self,
        message: Value,
        through: bool,
        mut usernames: VecDeque<String>,
        answer: Answer,
    ) {
        if let Some(username) = usernames.pop_front() {
            self.send(
                "searchPublicChat",
                json!({"username":username}),
                Some(Box::new(move |b, v| {
                    if v["@type"] == "chat" && v["id"] == message["chat_id"] {
                        b.safe_boundary(message, through, answer);
                    } else {
                        b.resolve_read(message, through, usernames, answer);
                    }
                })),
            );
        } else {
            let _ = answer
                .send(json!({"ok":true,"skipped":"Chat is not configured for dismissIsRead"}));
        }
    }
    fn transcribe_failed(&mut self, key: &str, message: &str, answer: Answer) {
        if let Some(e) = self
            .entries
            .get_mut(key)
            .filter(|e| e.notice["data"]["transcriptionState"] != "done")
        {
            merge(
                &mut e.notice["data"],
                &json!({"transcriptionState":"error","transcriptionError":trunc(message,4096)}),
            );
            self.show(key);
        }
        let _ = answer.send(json!({"ok":false,"error":message}));
    }
    fn transcribe(&mut self, key: String, e: Entry, answer: Answer) -> Result<()> {
        ensure!(
            e.notice["data"]["kind"] == "voice",
            "Transcription requires a visible voice message"
        );
        let state = text(&e.notice["data"]["transcriptionState"]);
        if ["pending", "done"].contains(&state) {
            let _=answer.send(json!({"ok":true,"message":if state=="pending"{"Transcription already requested"}else{"Transcript is ready"}}));
            return Ok(());
        }
        merge(
            &mut self.entries.get_mut(&key).unwrap().notice["data"],
            &json!({"transcriptionState":"pending","transcriptionError":""}),
        );
        self.show(&key);
        let chat = e.message["chat_id"].clone();
        let mid = e.message["id"].clone();
        self.send(
            "getMessageProperties",
            json!({"chat_id":chat,"message_id":mid}),
            Some(Box::new(move |b, v| {
                if !b.entries.contains_key(&key) {
                    let _ = answer
                        .send(json!({"ok":false,"error":"This notification is no longer active"}));
                } else if v["@type"] == "error" {
                    b.transcribe_failed(
                        &key,
                        v["message"]
                            .as_str()
                            .unwrap_or("Could not check transcription availability"),
                        answer,
                    );
                } else if v["can_recognize_speech"] != true {
                    b.transcribe_failed(
                        &key,
                        "Telegram transcription is unavailable for this message or account",
                        answer,
                    );
                } else {
                    b.send(
                        "recognizeSpeech",
                        json!({"chat_id":chat,"message_id":mid}),
                        Some(Box::new(move |b, v| {
                            if v["@type"] == "error" {
                                b.transcribe_failed(
                                    &key,
                                    v["message"]
                                        .as_str()
                                        .unwrap_or("Telegram could not transcribe this message"),
                                    answer,
                                );
                            } else {
                                let _ = answer
                                    .send(json!({"ok":true,"message":"Transcription requested"}));
                            }
                        })),
                    );
                }
            })),
        );
        Ok(())
    }
    fn demo_voice(&mut self, request: Value, answer: Answer) {
        self.send("getMe",json!({}),Some(Box::new(move|b,v|{let names=v["usernames"]["active_usernames"].as_array().into_iter().flatten().filter_map(Value::as_str).chain(v["username"].as_str());if v["@type"]!="user"||!names.into_iter().any(|s|s.eq_ignore_ascii_case("tijptjik")){let _=answer.send(json!({"ok":false,"error":"Demo recording requires the signed-in @tijptjik account"}));return}let id=v["id"].clone();b.send("createPrivateChat",json!({"user_id":id,"force":true}),Some(Box::new(move|b,v|{if v["@type"]!="chat"||v["type"]["@type"]!="chatTypePrivate"||v["type"]["user_id"]!=id{let _=answer.send(json!({"ok":false,"error":"Could not verify Saved Messages for @tijptjik"}));return}if let Err(e)=b.voice_reply(&request,&json!({"chat_id":v["id"],"id":null}),"demo-self".into(),answer.clone()){let _=answer.send(json!({"ok":false,"error":e.to_string()}));}})));})));
    }
    fn action(&mut self, request: Value, answer: Answer) {
        let result = (|| -> Result<()> {
            if request["action"] == "demoVoiceReply" {
                self.demo_voice(request.clone(), answer.clone());
                return Ok(());
            }
            let chat = identifier(&request["chatId"])?;
            let mid = identifier(&request["messageId"])?;
            let key = key(chat, mid);
            let e = self
                .entries
                .get(&key)
                .cloned()
                .context("This notification is no longer active")?;
            match text(&request["action"]) {
                "read" => self.read(chat, mid, answer.clone()),
                "safeRead" | "dismissRead" => {
                    let explicit = request["action"] == "safeRead";
                    let options = if explicit {
                        json!({"dismissIsRead":[],"dismissReadMode":"safe"})
                    } else {
                        crate::config::load(&self.root)?["telegram"].clone()
                    };
                    let through = options["dismissReadMode"] == "through";
                    let rules = options["dismissIsRead"].as_array().unwrap();
                    if explicit || rules.iter().any(|r| r == &json!(chat.to_string())) {
                        self.safe_boundary(e.message, through, answer.clone());
                    } else {
                        self.resolve_read(
                            e.message,
                            through,
                            rules
                                .iter()
                                .filter_map(|r| {
                                    r.as_str()
                                        .and_then(|s| s.strip_prefix('@'))
                                        .map(str::to_string)
                                })
                                .collect(),
                            answer.clone(),
                        );
                    }
                }
                "transcribe" => self.transcribe(key, e, answer.clone())?,
                "reply" => {
                    let s = request["text"]
                        .as_str()
                        .context("Reply must contain 1–4096 characters")?;
                    ensure!(
                        !s.trim().is_empty() && s.chars().count() <= 4096,
                        "Reply must contain 1–4096 characters"
                    );
                    let quote = request
                        .get("quote")
                        .unwrap_or(&Value::Bool(true))
                        .as_bool()
                        .context("Quote mode must be a boolean")?;
                    self.send("sendMessage",json!({"chat_id":chat,"topic_id":e.message["topic_id"],"reply_to":if quote{json!({"@type":"inputMessageReplyToMessage","message_id":mid})}else{Value::Null},"input_message_content":{"@type":"inputMessageText","text":{"@type":"formattedText","text":s,"entities":[]},"clear_draft":false}}),Some(checked(answer.clone())));
                }
                "voiceReply" => self.voice_reply(&request, &e.message, key, answer.clone())?,
                "open" => {
                    let a = answer.clone();
                    self.send("getMessageLink",json!({"chat_id":chat,"message_id":mid,"media_timestamp":0,"for_album":false,"in_message_thread":false}),Some(Box::new(move|b,v|{let link=text(&v["link"]);if v["@type"]=="messageLink"&&!link.is_empty(){launch_link(link,a,"");return}let c=&b.chats.get(&chat).unwrap_or(&Value::Null)["type"];if c["@type"]=="chatTypePrivate"{b.send("getUser",json!({"user_id":c["user_id"]}),Some(Box::new(move|_,v|{let name=v["usernames"]["active_usernames"].as_array().and_then(|a|a.first()).and_then(Value::as_str);let param=if let Some(name)=name{Some(("domain",name))}else{v["phone_number"].as_str().filter(|s|!s.is_empty()).map(|s|("phone",s))};if let Some((k,v))=param{launch_link(&format!("tg://resolve?{}",url::form_urlencoded::Serializer::new(String::new()).append_pair(k,v).finish()),a,"");}else{launch_link("tg://",a,"Opened Telegram; this private chat has no direct desktop link");}})));}else{launch_link("tg://",a,"Opened Telegram; this chat has no direct desktop link");}})));
                }
                _ => anyhow::bail!("Unknown Telegram action"),
            };
            Ok(())
        })();
        if let Err(e) = result {
            let _ = answer.send(json!({"ok":false,"error":e.to_string()}));
        }
    }
    fn auth(&mut self, state: Value) -> Result<()> {
        if self.auth_state == state {
            return Ok(());
        }
        self.auth_state = state.clone();
        let callback = || {
            Some(Box::new(|b: &mut Bridge, v: Value| {
                if v["@type"] == "error" {
                    b.auth_error = format!(
                        "Telegram rejected authentication: {}",
                        v["message"].as_str().unwrap_or("unknown error")
                    );
                }
            }) as Callback)
        };
        match text(&state["@type"]) {
            "authorizationStateWaitTdlibParameters" => {
                private_dir(&telegram_dir().join("db"))?;
                private_dir(&telegram_dir().join("files"))?;
                self.send("setTdlibParameters",json!({"database_directory":telegram_dir().join("db"),"files_directory":telegram_dir().join("files"),"database_encryption_key":self.creds.get("key").unwrap_or(&json!("")),"use_file_database":true,"use_chat_info_database":true,"use_message_database":true,"use_secret_chats":false,"api_id":self.creds["api_id"],"api_hash":self.creds["api_hash"],"system_language_code":"en","device_model":"Notjik","system_version":"Linux","application_version":"0.1"}),callback());
            }
            "authorizationStateReady" => self.ready = true,
            "authorizationStateClosed" => self.closed = true,
            "authorizationStateClosing" | "authorizationStateLoggingOut" => self.ready = false,
            kind => {
                ensure!(self.login,"Telegram sign-in is required. Stop the bridge and run notjikctl telegram login.");
                let(method,fields)=match kind{"authorizationStateWaitPhoneNumber"=>("setAuthenticationPhoneNumber",json!({"phone_number":prompt("Phone number (with country code): ",false)?})),"authorizationStateWaitCode"=>("checkAuthenticationCode",json!({"code":prompt("Telegram login code: ",true)?})),"authorizationStateWaitPassword"=>("checkAuthenticationPassword",json!({"password":prompt("Two-step verification password: ",true)?})),"authorizationStateWaitEmailAddress"=>("setAuthenticationEmailAddress",json!({"email_address":prompt("Login email address: ",false)?})),"authorizationStateWaitEmailCode"=>("checkAuthenticationEmailCode",json!({"code":{"@type":"emailAddressAuthenticationCode","code":prompt("Email verification code: ",true)?}})),"authorizationStateWaitOtherDeviceConfirmation"=>{println!("Confirm this sign-in from Telegram on another device: {}",text(&state["link"]));return Ok(())},_=>anyhow::bail!("This account needs an authentication step in the official Telegram app: {kind}")};
                self.send(method, fields, callback());
            }
        }
        Ok(())
    }
    fn presence(&mut self, status: &Value, ready: bool) {
        let desired = ready && status["lockState"] == "unlocked" && status["paused"] == false;
        if self.presence_pending == Some(desired)
            || (self.presence_pending.is_none() && self.online == Some(desired))
        {
            return;
        }
        self.presence_generation += 1;
        let generation = self.presence_generation;
        self.presence_pending = Some(desired);
        self.online = None;
        self.send(
            "setOption",
            json!({"name":"online","value":{"@type":"optionValueBoolean","value":desired}}),
            Some(Box::new(move |b, v| {
                if b.presence_generation != generation {
                    return;
                }
                if v["@type"] != "ok" {
                    b.presence_pending = None;
                    b.online = None;
                    return;
                }
                b.send(
                    "getOption",
                    json!({"name":"online"}),
                    Some(Box::new(move |b, v| {
                        if b.presence_generation == generation {
                            b.presence_pending = None;
                            b.online =
                                if v["@type"] == "optionValueBoolean" && v["value"] == desired {
                                    Some(desired)
                                } else {
                                    None
                                };
                        }
                    })),
                );
            })),
        );
    }
    fn prune_cache(&self) {
        let mut active = HashSet::new();
        for e in self.entries.values() {
            active.insert(text(&e.notice["data"]["source"]).to_string());
            for p in e.notice["data"]["parts"].as_array().into_iter().flatten() {
                active.insert(text(&p["source"]).to_string());
            }
        }
        let Ok(entries) = fs::read_dir(telegram_dir().join("rendered")) else {
            return;
        };
        let mut files = entries
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if !p.extension().is_some_and(|e| e == "gif" || e == "png") {
                    return None;
                }
                let m = e.metadata().ok()?;
                Some((m.modified().ok()?, m.len(), p))
            })
            .collect::<Vec<_>>();
        files.sort_by_key(|v| v.0);
        let mut total = files.iter().map(|v| v.1).sum::<u64>();
        for (time, size, p) in files {
            if !active.contains(&uri(&p).unwrap_or_default())
                && (time.elapsed().unwrap_or_default() > Duration::from_secs(7 * 86400)
                    || total > 256 * 1024 * 1024)
                && fs::remove_file(p).is_ok()
            {
                total = total.saturating_sub(size);
            }
        }
    }
}
fn file_size(v: &Value) -> u64 {
    [
        v["size"].as_u64(),
        v["expected_size"].as_u64(),
        v["local"]["downloaded_size"].as_u64(),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(0)
}
fn download(id: i64, priority: i32) -> Value {
    json!({"file_id":id,"priority":priority,"offset":0,"limit":MAX_FILE+1,"synchronous":false})
}
fn launch_link(link: &str, answer: Answer, message: &str) {
    let result = if !(link.starts_with("https://t.me/")
        || link.starts_with("tg://resolve?")
        || link == "tg://")
    {
        json!({"ok":false,"error":"Unexpected Telegram message link"})
    } else if Command::new("xdg-open")
        .arg(link)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
    {
        json!({"ok":true,"message":message})
    } else {
        json!({"ok":false,"error":"Could not open Telegram"})
    };
    let _ = answer.send(result);
}
fn prompt(label: &str, secret: bool) -> Result<String> {
    print!("{label}");
    std::io::stdout().flush()?;
    let fd = std::io::stdin().as_raw_fd();
    let mut old = std::mem::MaybeUninit::<libc::termios>::uninit();
    let have = unsafe { libc::tcgetattr(fd, old.as_mut_ptr()) } == 0;
    ensure!(!secret || have, "Secret input requires a terminal");
    let old = if have {
        Some(unsafe { old.assume_init() })
    } else {
        None
    };
    struct Restore(i32, Option<libc::termios>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(t) = &self.1 {
                unsafe {
                    libc::tcsetattr(self.0, libc::TCSAFLUSH, t);
                }
                println!();
            }
        }
    }
    let _restore = Restore(fd, if secret { old } else { None });
    if secret {
        let mut t = old.unwrap();
        t.c_lflag &= !libc::ECHO;
        ensure!(
            unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &t) } == 0,
            "Cannot hide secret input"
        );
    }
    let mut s = String::new();
    ensure!(std::io::stdin().read_line(&mut s)? > 0, "Input closed");
    Ok(s.trim_end_matches(['\r', '\n']).to_string())
}
pub fn run_bridge(root: &Path, login: bool) -> Result<()> {
    STOP.store(false, Ordering::Relaxed);
    unsafe {
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
    }
    private_dir(&telegram_dir())?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(telegram_dir().join("session.lock"))?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "The Telegram session is already in use. Stop notjik-telegram.service before signing in."
    );
    let mut b = Bridge::new(root, login)?;
    b.send("getOption", json!({"name":"version"}), None);
    let (mut configured, mut listener) = (false, None);
    let (tx, requests) = mpsc::sync_channel::<(Value, Answer)>(32);
    let mut heartbeat = Instant::now();
    let mut cleanup = Instant::now();
    let result = (|| -> Result<()> {
        while !STOP.load(Ordering::Relaxed) && !b.closed {
            let event = b.receive()?;
            b.poll_media();
            if !b.auth_error.is_empty() {
                let error = std::mem::take(&mut b.auth_error);
                ensure!(login, "{error}");
                eprintln!("{error}");
                let state = std::mem::replace(&mut b.auth_state, Value::Null);
                b.auth(state)?;
            }
            if let Some(v) = event {
                match text(&v["@type"]) {
                    "updateAuthorizationState" => b.auth(v["authorization_state"].clone())?,
                    "updateConnectionState" => {
                        b.connected = v["state"]["@type"] == "connectionStateReady";
                        heartbeat = Instant::now();
                    }
                    _ => {
                        if b.ready && !login {
                            b.handle(v)
                        }
                    }
                }
            }
            if b.ready && login {
                println!("Telegram sign-in saved. Start notjik-telegram.service to receive notifications.");
                break;
            }
            if b.ready && !configured {
                configured = true;
                b.recover_uploads();
                for (name, value) in [
                    ("notification_group_count_max", 25),
                    ("notification_group_size_max", 10),
                ] {
                    b.send(
                        "setOption",
                        json!({"name":name,"value":{"@type":"optionValueInteger","value":value}}),
                        Some(Box::new(move |b, v| {
                            if v["@type"] == "ok" {
                                b.options.insert(name.into());
                            } else {
                                b.auth_error = "TDLib notification options were rejected".into();
                            }
                        })),
                    );
                }
                let p = socket_path()?;
                private_dir(p.parent().unwrap())?;
                if p.exists() {
                    fs::remove_file(&p)?;
                }
                let l = UnixListener::bind(&p)?;
                fs::set_permissions(&p, fs::Permissions::from_mode(0o600))?;
                l.set_nonblocking(true)?;
                listener = Some(l);
            }
            if let Some(l) = &listener {
                for _ in 0..32 {
                    match l.accept() {
                        Ok((s, _)) => {
                            let tx = tx.clone();
                            std::thread::spawn(move || serve(s, tx));
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e.into()),
                    }
                }
            }
            for (request, answer) in requests.try_iter() {
                if request["action"] == "status" {
                    let _=answer.send(json!({"ok":true,"authorized":b.ready,"connected":b.connected,"online":b.online,"active":b.entries.len(),"delivery":b.delivery}));
                } else if !b.ready || !b.connected {
                    let _ = answer.send(json!({"ok":false,"error":"Telegram is reconnecting"}));
                } else {
                    b.action(request, answer);
                }
            }
            let now = Instant::now();
            if !login && now >= heartbeat {
                heartbeat = now + Duration::from_secs(1);
                let retry = b
                    .order
                    .iter()
                    .filter(|k| !b.entries[*k].published)
                    .cloned()
                    .collect::<Vec<_>>();
                for k in retry {
                    b.show(&k);
                }
                let ready = b.ready && b.connected && b.options.len() == 2;
                let active =
                    ready && b.online == Some(true) && b.entries.values().all(|e| e.published);
                let status =
                    ipc(root, &json!({"op":"telegramBridge","active":active})).unwrap_or(json!({}));
                if b.ready {
                    b.presence(&status, ready);
                }
            }
            if b.ready && !login && now >= cleanup {
                cleanup = now + Duration::from_secs(3600);
                b.prune_cache();
                b.send("optimizeStorage",json!({"size":256*1024*1024,"ttl":7*86400,"count":10000,"immunity_delay":3600,"file_types":[],"chat_ids":[],"exclude_chat_ids":[],"return_deleted_file_statistics":false,"chat_limit":0}),None);
            }
        }
        Ok(())
    })();
    if !login && b.ready {
        b.presence(&json!({}), false);
    }
    if listener.is_some() {
        drop(listener);
        if let Ok(p) = socket_path() {
            let _ = fs::remove_file(p);
        }
    }
    if !login {
        let _ = ipc(root, &json!({"op":"telegramBridge","active":false}));
    }
    b.send("close", json!({}), None);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !b.closed && Instant::now() < deadline {
        if let Ok(Some(v)) = b.receive() {
            if v["@type"] == "updateAuthorizationState" {
                b.closed = v["authorization_state"]["@type"] == "authorizationStateClosed";
            }
        }
    }
    result
}
fn serve(mut stream: UnixStream, requests: mpsc::SyncSender<(Value, Answer)>) {
    let mut voice = false;
    let result = (|| -> Result<Value> {
        stream.set_read_timeout(Some(Duration::from_secs(35)))?;
        stream.set_write_timeout(Some(Duration::from_secs(35)))?;
        let mut raw = vec![];
        BufReader::new(&stream)
            .take(32769)
            .read_until(b'\n', &mut raw)?;
        ensure!(raw.len() <= 32768, "Telegram request too large");
        let request: Value = serde_json::from_slice(&raw)?;
        ensure!(request.is_object(), "Telegram request must be an object");
        voice = request["action"] == "voiceReply";
        let (tx, rx) = mpsc::sync_channel(1);
        requests
            .try_send((request, tx))
            .map_err(|_| anyhow::anyhow!("Telegram bridge did not accept the request"))?;
        Ok(rx.recv_timeout(Duration::from_secs(32))?)
    })();
    let response=result.unwrap_or_else(|_|json!({"ok":false,"uncertain":voice,"error":if voice{"No confirmation from Telegram; check the chat before retrying"}else{"Telegram bridge did not accept the request"}}));
    let _ = writeln!(stream, "{response}");
}
#[cfg(test)]
mod tests {
    use super::*;
    fn bridge() -> Bridge {
        Bridge::with_td(
            Path::new("/nonexistent/notjik-test"),
            false,
            json!({}),
            Td::mock(),
        )
    }
    fn respond(b: &mut Bridge, index: usize, v: Value) {
        let call = &Td::calls()[index];
        let (_, cb) = b
            .pending
            .remove(text(&call["@extra"]))
            .expect("pending callback");
        cb(b, v);
    }
    fn message() -> Value {
        json!({"chat_id":123,"id":456})
    }
    #[test]
    fn safe_read_does_not_cross_unread_boundary() {
        let mut b = bridge();
        let (tx, rx) = mpsc::sync_channel(1);
        b.safe_boundary(message(), false, tx);
        assert_eq!(Td::calls()[0]["@type"], "getChat");
        respond(
            &mut b,
            0,
            json!({"@type":"chat","id":123,"last_read_inbox_message_id":100,"unread_count":3}),
        );
        assert_eq!(Td::calls()[1]["@type"], "getChatHistory");
        respond(
            &mut b,
            1,
            json!({"@type":"messages","messages":[{"id":456,"chat_id":123},{"id":200,"chat_id":123}]}),
        );
        assert!(rx.recv().unwrap()["skipped"].is_string());
        assert!(!Td::calls().iter().any(|c| c["@type"] == "viewMessages"));
    }
    #[test]
    fn safe_read_verified_predecessor_and_only_unread() {
        for single in [false, true] {
            let mut b = bridge();
            let (tx, rx) = mpsc::sync_channel(1);
            b.safe_boundary(message(), false, tx);
            respond(
                &mut b,
                0,
                json!({"@type":"chat","id":123,"last_read_inbox_message_id":100,"unread_count":if single{1}else{2},"last_message":{"id":456}}),
            );
            if !single {
                respond(
                    &mut b,
                    1,
                    json!({"@type":"messages","messages":[{"id":456,"chat_id":123},{"id":100,"chat_id":123}]}),
                );
            }
            let index = Td::calls().len() - 1;
            assert_eq!(Td::calls()[index]["@type"], "viewMessages");
            assert_eq!(Td::calls()[index]["message_ids"], json!([456]));
            respond(&mut b, index, json!({"@type":"ok"}));
            assert_eq!(rx.recv().unwrap()["ok"], true);
        }
    }
    #[test]
    fn topic_and_wrong_chat_fail_closed() {
        let mut b = bridge();
        let (tx, rx) = mpsc::sync_channel(1);
        let mut m = message();
        m["message_thread_id"] = json!(7);
        b.safe_boundary(m, false, tx);
        assert!(Td::calls().is_empty());
        assert!(rx.recv().unwrap()["skipped"].is_string());
        let (tx, rx) = mpsc::sync_channel(1);
        b.safe_boundary(message(), false, tx);
        respond(
            &mut b,
            0,
            json!({"@type":"chat","id":999,"unread_count":1,"last_message":{"id":456}}),
        );
        assert!(rx.recv().unwrap()["skipped"].is_string());
        assert_eq!(Td::calls().len(), 1);
    }
    #[test]
    fn presence_verified_and_stale_ack_ignored() {
        let mut b = bridge();
        b.presence(&json!({"lockState":"unlocked","paused":false}), true);
        assert_eq!(Td::calls()[0]["value"]["value"], true);
        assert_eq!(b.online, None);
        b.presence(&json!({}), true);
        respond(&mut b, 0, json!({"@type":"ok"}));
        assert_eq!(Td::calls().len(), 2);
        respond(&mut b, 1, json!({"@type":"ok"}));
        assert_eq!(Td::calls()[2]["@type"], "getOption");
        respond(
            &mut b,
            2,
            json!({"@type":"optionValueBoolean","value":false}),
        );
        assert_eq!(b.online, Some(false));
        b.presence(&json!({}), true);
        assert_eq!(Td::calls().len(), 3);
    }
    #[test]
    fn reply_validates_identity_quote_and_text() {
        let mut b = bridge();
        let k = key(123, 456);
        b.entries.insert(
            k.clone(),
            Entry {
                message: message(),
                notice: json!({"data":{"kind":"text"}}),
                preview: true,
                published: true,
                file: 0,
                avatar: 0,
                generation: 1,
            },
        );
        for r in [
            json!({"action":"reply","chatId":"123","messageId":"456","text":""}),
            json!({"action":"reply","chatId":"123","messageId":"456","text":"hi","quote":"false"}),
            json!({"action":"reply","chatId":true,"messageId":"456","text":"hi"}),
            json!({"action":"reply","chatId":"123","messageId":"999","text":"hi"}),
        ] {
            let (tx, rx) = mpsc::sync_channel(1);
            b.action(r, tx);
            assert_eq!(rx.recv().unwrap()["ok"], false);
        }
        assert!(Td::calls().is_empty());
        let (tx, rx) = mpsc::sync_channel(1);
        b.action(
            json!({"action":"reply","chatId":"123","messageId":"456","text":"hello","quote":false}),
            tx,
        );
        assert_eq!(Td::calls()[0]["@type"], "sendMessage");
        assert!(Td::calls()[0]["reply_to"].is_null());
        respond(&mut b, 0, json!({"@type":"message","id":789}));
        assert_eq!(rx.recv().unwrap()["ok"], true);
    }
    #[test]
    fn filtering_does_not_replay_old_or_outgoing() {
        let mut b = bridge();
        let n = |date: f64, outgoing: bool| json!({"id":1,"date":date,"type":{"@type":"notificationTypeNewMessage","message":{"id":456,"is_outgoing":outgoing},"show_preview":true}});
        b.add(1, 123, n(now() - 100., false));
        b.add(1, 123, n(now(), true));
        b.add(
            1,
            123,
            json!({"id":1,"type":{"@type":"notificationTypeNewCall"}}),
        );
        assert!(b.entries.is_empty());
        assert_eq!(b.delivery["ignoredOld"], 1);
        assert_eq!(b.delivery["ignoredOutgoing"], 1);
        assert_eq!(b.delivery["ignoredType"], 1);
        assert!(Td::calls().is_empty());
    }
    #[test]
    fn pending_requests_timeout_without_success() {
        let mut b = bridge();
        let (tx, rx) = mpsc::sync_channel(1);
        b.read(123, 456, tx);
        for (deadline, _) in b.pending.values_mut() {
            *deadline = Instant::now() - Duration::from_secs(1);
        }
        b.receive().unwrap();
        let answer = rx.recv().unwrap();
        assert_eq!(answer["ok"], false);
        assert!(text(&answer["error"]).contains("timed out"));
        assert!(b.pending.is_empty());
    }
    #[test]
    fn named_ids_preserve_negative_chat() {
        assert_eq!(key(-123, 456), "chat-123/456");
        assert_eq!(
            notice_id(&key(-123, 456)),
            "named-Telegram%20via%20Notjik/chat-123%2F456"
        );
    }
}
