//! Saved Messages delivery on the bridge's single-owner TDLib event loop.
use super::*;
use crate::telegram_clipboard;

struct Job {
    directory: PathBuf,
    contents: VecDeque<Value>,
    chat: i64,
    sent: usize,
    deadline: Instant,
    warned: bool,
}
pub(super) struct Outbox {
    root: PathBuf,
    jobs: HashMap<String, Job>,
    pending: HashMap<(i64, i64), String>,
    #[cfg(test)]
    notices: Vec<Value>,
}
impl Default for Outbox {
    fn default() -> Self {
        Self {
            root: telegram_dir().join("clipboard-outbox"),
            jobs: HashMap::new(),
            pending: HashMap::new(),
            #[cfg(test)]
            notices: vec![],
        }
    }
}
impl Bridge {
    fn clipboard_notice(&mut self, success: bool, body: &str) {
        let notice = telegram_clipboard::notice(success, body);
        #[cfg(test)]
        self.clipboard.notices.push(notice);
        #[cfg(not(test))]
        {
            let _ = ipc(&self.root, &json!({"op":"send","notice":notice}));
        }
    }
    pub(super) fn clipboard_start(&mut self, token: &str) -> Result<()> {
        ensure!(
            !self.clipboard.jobs.contains_key(token),
            "This clipboard snapshot is already being sent"
        );
        let (directory, contents) = telegram_clipboard::contents(&self.clipboard.root, token)?;
        // Claim the snapshot durably before any TDLib request. A restart or a
        // repeated control request must never resend a possibly delivered item.
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.join("claimed"))?
            .sync_all()?;
        fs::File::open(&directory)?.sync_all()?;
        self.clipboard.jobs.insert(
            token.into(),
            Job {
                directory,
                contents: contents.into(),
                chat: 0,
                sent: 0,
                deadline: Instant::now() + Duration::from_secs(90),
                warned: false,
            },
        );
        let token = token.to_string();
        self.send(
            "getMe",
            json!({}),
            Some(Box::new(move |b, user| {
                let Some(id) = user["id"].as_i64().filter(|id| *id > 0) else {
                    b.clipboard_fail(&token, "Could not identify your Telegram account");
                    return;
                };
                if user["@type"] != "user" {
                    b.clipboard_fail(&token, "Could not identify your Telegram account");
                    return;
                }
                b.send(
                    "createPrivateChat",
                    json!({"user_id":id,"force":true}),
                    Some(Box::new(move |b, chat| {
                        let destination = chat["id"].as_i64().filter(|id| *id != 0);
                        if chat["@type"] != "chat"
                            || chat["type"]["@type"] != "chatTypePrivate"
                            || chat["type"]["user_id"] != id
                            || destination.is_none()
                        {
                            b.clipboard_fail(&token, "Could not verify Saved Messages");
                            return;
                        }
                        if let Some(job) = b.clipboard.jobs.get_mut(&token) {
                            job.chat = destination.unwrap();
                            b.clipboard_next(&token);
                        }
                    })),
                );
            })),
        );
        Ok(())
    }
    fn clipboard_fail(&mut self, token: &str, body: &str) {
        if let Some(job) = self.clipboard.jobs.remove(token) {
            self.clipboard.pending.retain(|_, v| v != token);
            let prefix = if job.sent > 0 {
                format!("{} item(s) saved. ", job.sent)
            } else {
                String::new()
            };
            self.clipboard_notice(false, &(prefix + body));
        }
        // Retain snapshots on errors, including missing acknowledgements.
    }
    fn clipboard_next(&mut self, token: &str) {
        let Some(job) = self.clipboard.jobs.get_mut(token) else {
            return;
        };
        let Some(content) = job.contents.pop_front() else {
            let job = self.clipboard.jobs.remove(token).unwrap();
            let _ = fs::remove_dir_all(job.directory);
            self.clipboard_notice(true, "");
            return;
        };
        job.deadline = Instant::now() + Duration::from_secs(90);
        let chat = job.chat;
        let token = token.to_string();
        self.send(
            "sendMessage",
            json!({"chat_id":chat,"input_message_content":content}),
            Some(Box::new(move |b, message| {
                if message["@type"] != "message"
                    || message["chat_id"] != chat
                    || message["id"].as_i64().filter(|id| *id != 0).is_none()
                {
                    b.clipboard_fail(
                        &token,
                        "Telegram did not confirm the send; check Saved Messages before retrying",
                    );
                } else if message["sending_state"]["@type"] == "messageSendingStateFailed" {
                    b.clipboard_fail(
                        &token,
                        "Telegram could not send this item; check Saved Messages before retrying",
                    );
                } else if !message["sending_state"].is_null() {
                    b.clipboard
                        .pending
                        .insert((chat, message["id"].as_i64().unwrap()), token);
                } else if let Some(job) = b.clipboard.jobs.get_mut(&token) {
                    job.sent += 1;
                    b.clipboard_next(&token);
                }
            })),
        );
    }
    pub(super) fn clipboard_event(&mut self, event: &Value) {
        let success = event["@type"] == "updateMessageSendSucceeded";
        if !success && event["@type"] != "updateMessageSendFailed" {
            return;
        }
        let (Some(chat), Some(old)) = (
            event["message"]["chat_id"].as_i64(),
            event["old_message_id"].as_i64(),
        ) else {
            return;
        };
        let Some(token) = self.clipboard.pending.remove(&(chat, old)) else {
            return;
        };
        if success {
            if let Some(job) = self.clipboard.jobs.get_mut(&token) {
                job.sent += 1;
                self.clipboard_next(&token);
            }
        } else {
            self.clipboard_fail(
                &token,
                "Telegram could not send this item; check Saved Messages before retrying",
            );
        }
    }
    pub(super) fn clipboard_poll(&mut self) {
        let mut warnings = 0;
        for job in self.clipboard.jobs.values_mut() {
            if !job.warned && Instant::now() > job.deadline {
                job.warned = true;
                warnings += 1;
            }
        }
        for _ in 0..warnings {
            self.clipboard_notice(
                false,
                "Send is still pending; check Saved Messages before retrying",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{bridge, respond};
    use super::*;
    fn fixture(types: &str, data: &[u8]) -> (tempfile::TempDir, Bridge, String) {
        let temp = tempfile::tempdir().unwrap();
        let token = telegram_clipboard::snapshot(temp.path(), |mime| {
            Ok(if mime.is_none() {
                types.as_bytes()
            } else {
                data
            }
            .to_vec())
        })
        .unwrap();
        let mut b = bridge();
        b.clipboard.root = temp.path().into();
        b.ready = true;
        b.connected = true;
        (temp, b, token)
    }
    fn resolve(b: &mut Bridge) {
        respond(b, 0, json!({"@type":"user","id":42}));
        assert_eq!(Td::calls()[1]["user_id"], 42);
        respond(
            b,
            1,
            json!({"@type":"chat","id":42,"type":{"@type":"chatTypePrivate","user_id":42}}),
        );
    }
    fn confirm(b: &mut Bridge, index: usize, id: i64) {
        respond(
            b,
            index,
            json!({"@type":"message","chat_id":42,"id":id,"sending_state":{"@type":"messageSendingStatePending"}}),
        );
        assert!(!b
            .clipboard
            .notices
            .iter()
            .any(|n| n["title"] == "Saved to Telegram"));
        b.handle(json!({"@type":"updateMessageSendSucceeded","old_message_id":id,"message":{"chat_id":42}}));
    }
    #[test]
    fn literal_text_waits_for_confirmation_and_cleans_up() {
        let text = "**literal** <tag> 中文\nsecond line\n";
        let (temp, mut b, token) = fixture("text/plain", text.as_bytes());
        let (tx, rx) = mpsc::sync_channel(1);
        b.action(json!({"action":"saveClipboard","token":token}), tx);
        assert_eq!(rx.recv().unwrap(), json!({"ok":true,"queued":true}));
        resolve(&mut b);
        let content = &Td::calls()[2]["input_message_content"];
        assert_eq!(content["text"]["text"], text);
        assert_eq!(content["text"]["entities"], json!([]));
        assert!(temp.path().join(&token).exists());
        confirm(&mut b, 2, 99);
        assert!(!temp.path().join(token).exists());
        assert_eq!(
            b.clipboard.notices,
            vec![telegram_clipboard::notice(true, "")]
        );
    }
    #[test]
    fn wrong_destination_never_sends() {
        let (temp, mut b, token) = fixture("text/plain", b"synthetic");
        b.clipboard_start(&token).unwrap();
        respond(&mut b, 0, json!({"@type":"user","id":42}));
        respond(
            &mut b,
            1,
            json!({"@type":"chat","id":7,"type":{"@type":"chatTypePrivate","user_id":7}}),
        );
        assert_eq!(Td::calls().len(), 2);
        assert_eq!(b.clipboard.notices[0]["timeout"], 5000);
        assert!(temp.path().join(token).exists());
    }
    #[test]
    fn images_are_documents_and_duplicate_requests_never_resend() {
        let (temp, mut b, token) = fixture("image/png\ntext/plain", b"synthetic png");
        b.clipboard_start(&token).unwrap();
        assert!(b.clipboard_start(&token).is_err());
        resolve(&mut b);
        let content = &Td::calls()[2]["input_message_content"];
        assert_eq!(content["@type"], "inputMessageDocument");
        let path = PathBuf::from(content["document"]["document"]["path"].as_str().unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"synthetic png");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let mut restarted = bridge();
        restarted.clipboard.root = temp.path().into();
        assert!(restarted.clipboard_start(&token).is_err());
        assert!(Td::calls().is_empty());
    }
    #[test]
    fn timeout_failure_retains_snapshot_and_never_retries() {
        let (temp, mut b, token) = fixture("text/plain", b"synthetic");
        b.clipboard_start(&token).unwrap();
        resolve(&mut b);
        respond(
            &mut b,
            2,
            json!({"@type":"message","chat_id":42,"id":99,"sending_state":{"@type":"messageSendingStatePending"}}),
        );
        b.clipboard.jobs.get_mut(&token).unwrap().deadline =
            Instant::now() - Duration::from_secs(1);
        b.clipboard_poll();
        b.clipboard_poll();
        assert_eq!(b.clipboard.notices.len(), 1);
        assert_eq!(Td::calls().len(), 3);
        b.handle(
            json!({"@type":"updateMessageSendFailed","old_message_id":99,"message":{"chat_id":42}}),
        );
        assert_eq!(b.clipboard.notices.len(), 2);
        assert!(temp.path().join(token).exists());
        assert!(b.clipboard.jobs.is_empty());
    }
    #[test]
    fn all_files_must_confirm_and_partial_failure_is_reported() {
        let source = tempfile::tempdir().unwrap();
        let paths = [source.path().join("one.txt"), source.path().join("two.txt")];
        for p in &paths {
            fs::write(p, b"synthetic").unwrap();
        }
        let uris = paths
            .iter()
            .map(|p| uri(p).unwrap())
            .collect::<Vec<_>>()
            .join("\r\n");
        let (temp, mut b, token) = fixture("text/uri-list", uris.as_bytes());
        fs::remove_file(&paths[0]).unwrap();
        b.clipboard_start(&token).unwrap();
        resolve(&mut b);
        confirm(&mut b, 2, 10);
        assert!(b.clipboard.notices.is_empty());
        assert!(temp.path().join(&token).exists());
        respond(
            &mut b,
            3,
            json!({"@type":"error","message":"synthetic failure"}),
        );
        assert!(b.clipboard.notices[0]["body"]
            .as_str()
            .unwrap()
            .starts_with("1 item(s) saved."));
        assert!(temp.path().join(token).exists());
    }
    #[test]
    fn delayed_success_after_pending_notice_is_still_confirmed() {
        let (temp, mut b, token) = fixture("text/plain", b"synthetic");
        b.clipboard_start(&token).unwrap();
        resolve(&mut b);
        b.clipboard.jobs.get_mut(&token).unwrap().deadline =
            Instant::now() - Duration::from_secs(1);
        b.clipboard_poll();
        confirm(&mut b, 2, 99);
        assert_eq!(b.clipboard.notices.len(), 2);
        assert_eq!(b.clipboard.notices[1]["timeout"], 1500);
        assert!(!temp.path().join(token).exists());
    }
}
