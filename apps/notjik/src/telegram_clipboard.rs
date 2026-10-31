//! Private clipboard snapshots. The bridge owns delivery; the CLI never retries.
use crate::util::*;
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};
pub const MAX_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_FILES: usize = 10;

pub fn notice(success: bool, body: &str) -> Value {
    json!({"app":"Telegram Saved Messages", "title":if success {"Saved to Telegram"} else {"Could not save to Telegram"},
        "body":body,"timeout":if success {1500} else {5000},"urgency":"normal","transient":true})
}

fn clipboard(mime: Option<&str>) -> Result<Vec<u8>> {
    let mut cmd = Command::new("wl-paste");
    if let Some(mime) = mime {
        cmd.args(["--no-newline", "--type", mime]);
    } else {
        cmd.arg("--list-types");
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("Clipboard is empty or unavailable")?;
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let started = std::time::Instant::now();
    let result = rx.recv_timeout(Duration::from_secs(5));
    let mut exited = None;
    if matches!(&result, Ok(Ok(bytes)) if bytes.len() as u64 <= MAX_BYTES) {
        while started.elapsed() < Duration::from_secs(5) {
            if let Ok(Some(status)) = child.try_wait() {
                exited = Some(status);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    // Also reap a producer that leaves stdout open or exceeds the size bound.
    let _ = child.kill();
    let status = child.wait();
    let status = exited.map(Ok).unwrap_or(status);
    let _ = reader.join();
    let bytes = result.context("Clipboard is empty or unavailable")??;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "Clipboard exceeds the 20 MiB limit"
    );
    ensure!(status?.success(), "Clipboard is empty or unavailable");
    Ok(bytes)
}

pub fn snapshot(
    root: &Path,
    mut read: impl FnMut(Option<&str>) -> Result<Vec<u8>>,
) -> Result<String> {
    let types = String::from_utf8(read(None)?).context("Unsupported clipboard format")?;
    let has = |kind: &str| types.lines().any(|line| line == kind);
    let mut files = Vec::<(String, Vec<u8>)>::new();
    let mut text = None;
    if has("text/uri-list") || has("x-special/gnome-copied-files") {
        let gnome = !has("text/uri-list");
        let mime = if gnome {
            "x-special/gnome-copied-files"
        } else {
            "text/uri-list"
        };
        let data = String::from_utf8(read(Some(mime))?).context("Invalid file selection")?;
        let mut total = 0;
        for line in data
            .lines()
            .skip(usize::from(gnome))
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let uri = url::Url::parse(line).map_err(|_| {
                anyhow::anyhow!("Only local files can be sent from a file selection")
            })?;
            ensure!(
                uri.scheme() == "file" && matches!(uri.host_str(), None | Some("localhost")),
                "Only local files can be sent from a file selection"
            );
            let path = uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("Invalid local file selection"))?;
            let file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
                .context("Could not read a selected file")?;
            ensure!(
                file.metadata()?.is_file(),
                "Copy files, not folders, to send them to Telegram"
            );
            let mut data = Vec::new();
            file.take(MAX_BYTES + 1)
                .read_to_end(&mut data)
                .context("Could not read a selected file")?;
            total += data.len() as u64;
            ensure!(
                files.len() < MAX_FILES && total <= MAX_BYTES,
                "Copy at most 10 files totaling 20 MiB"
            );
            files.push((
                path.file_name()
                    .context("Invalid filename")?
                    .to_string_lossy()
                    .into_owned(),
                data,
            ));
        }
    } else if let Some((mime, ext)) = [
        ("image/png", "png"),
        ("image/jpeg", "jpg"),
        ("image/webp", "webp"),
        ("image/gif", "gif"),
    ]
    .into_iter()
    .find(|(mime, _)| has(mime))
    {
        files.push((format!("clipboard.{ext}"), read(Some(mime))?));
    } else {
        let mime = [
            "text/plain;charset=utf-8",
            "text/plain",
            "UTF8_STRING",
            "STRING",
        ]
        .into_iter()
        .find(|m| has(m))
        .context("Clipboard has no supported text, image, or files")?;
        let data = read(Some(mime))?;
        ensure!(
            data.len() as u64 <= MAX_BYTES,
            "Clipboard exceeds the 20 MiB limit"
        );
        let value = String::from_utf8(data).context("Clipboard text is not UTF-8")?;
        ensure!(!value.trim().is_empty(), "Clipboard is empty");
        if value.encode_utf16().count() > 4096 {
            files.push(("clipboard.txt".into(), value.into_bytes()));
        } else {
            text = Some(value);
        }
    }
    ensure!(
        text.is_some() || !files.is_empty(),
        "Clipboard file selection is empty"
    );
    ensure!(
        files.iter().map(|(_, data)| data.len() as u64).sum::<u64>() <= MAX_BYTES,
        "Clipboard exceeds the 20 MiB limit"
    );
    private_dir(root)?;
    let mut random = [0; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let token = random
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let directory = root.join(&token);
    // Do not overwrite an existing snapshot, even in the event of a collision.
    fs::DirBuilder::new().mode(0o700).create(&directory)?;
    let result = (|| -> Result<()> {
        let mut items = Vec::new();
        if let Some(text) = text {
            items.push(json!({"text":text}));
        }
        for (index, (name, data)) in files.into_iter().enumerate() {
            let relative = PathBuf::from(index.to_string()).join(name);
            private_write(&directory.join(&relative), &data)?;
            items.push(json!({"file":relative}));
        }
        private_write(&directory.join("items.json"), &serde_json::to_vec(&items)?)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&directory);
    }
    result?;
    Ok(token)
}
use std::os::unix::fs::DirBuilderExt;

pub fn contents(root: &Path, token: &str) -> Result<(PathBuf, Vec<Value>)> {
    ensure!(
        token.len() == 32
            && token
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Invalid clipboard snapshot"
    );
    let directory = root.join(token);
    ensure!(
        !fs::symlink_metadata(&directory)?.is_symlink(),
        "Invalid clipboard snapshot"
    );
    let directory = directory.canonicalize()?;
    let manifest = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(directory.join("items.json"))?;
    ensure!(manifest.metadata()?.is_file(), "Invalid clipboard snapshot");
    let mut raw = Vec::new();
    manifest.take(131073).read_to_end(&mut raw)?;
    ensure!(raw.len() <= 131072, "Invalid clipboard snapshot");
    let items: Value = serde_json::from_slice(&raw)?;
    let items = items.as_array().context("Invalid clipboard snapshot")?;
    ensure!(
        (1..=MAX_FILES).contains(&items.len()),
        "Invalid clipboard snapshot"
    );
    let mut result = Vec::new();
    let mut total = 0;
    for item in items {
        if let Some(text) = item["text"].as_str() {
            ensure!(
                !text.trim().is_empty() && text.encode_utf16().count() <= 4096,
                "Invalid clipboard text"
            );
            total += text.len() as u64;
            result.push(json!({"@type":"inputMessageText","text":{"@type":"formattedText","text":text,"entities":[]},"clear_draft":false}));
        } else {
            let path = directory
                .join(
                    item["file"]
                        .as_str()
                        .context("Invalid clipboard attachment")?,
                )
                .canonicalize()?;
            let meta = fs::metadata(&path)?;
            ensure!(
                path.starts_with(&directory) && meta.is_file(),
                "Invalid clipboard attachment"
            );
            total += meta.len();
            result.push(json!({"@type":"inputMessageDocument","document":{"@type":"inputDocument","document":{"@type":"inputFileLocal","path":path},"disable_content_type_detection":false}}));
        }
    }
    ensure!(total <= MAX_BYTES, "Clipboard exceeds the 20 MiB limit");
    Ok((directory, result))
}

pub fn save(root: &Path) -> Result<()> {
    save_with(
        &telegram_dir().join("clipboard-outbox"),
        crate::telegram::socket_request,
        |outbox| snapshot(outbox, clipboard),
        |notice| {
            let _ = ipc(root, &json!({"op":"send", "notice":notice}));
        },
    )
}
fn save_with(
    outbox: &Path,
    mut request: impl FnMut(&Value) -> Result<Value>,
    capture: impl FnOnce(&Path) -> Result<String>,
    mut publish: impl FnMut(Value),
) -> Result<()> {
    let result = (|| -> Result<()> {
        let status = request(&json!({"action":"status"}))
            .context("Run notjikctl telegram login, then start notjik-telegram.service")?;
        ensure!(
            status["authorized"] == true && status["connected"] == true,
            "Telegram is not connected"
        );
        let token = capture(outbox)?;
        let response = request(&json!({"action":"saveClipboard","token":token}))
            .context("No confirmation from Telegram; check Saved Messages before retrying")?;
        if response["ok"] != true {
            if response["uncertain"] != true {
                let _ = fs::remove_dir_all(outbox.join(token));
            }
            // Never echo an arbitrary bridge error or a clipboard path into a toast.
            bail!("Telegram did not accept the clipboard; check Saved Messages before retrying");
        }
        Ok(())
    })();
    if let Err(error) = &result {
        publish(notice(false, &error.to_string()));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    fn capture(root: &Path, types: &str, data: &[u8]) -> Result<String> {
        snapshot(root, |mime| {
            Ok(if mime.is_none() {
                types.as_bytes()
            } else {
                data
            }
            .to_vec())
        })
    }
    #[test]
    fn unicode_limit_and_private_permissions() {
        let temp = tempfile::tempdir().unwrap();
        let token = capture(temp.path(), "text/plain", "🌱".repeat(2048).as_bytes()).unwrap();
        let (dir, items) = contents(temp.path(), &token).unwrap();
        assert_eq!(items[0]["@type"], "inputMessageText");
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(dir.join("items.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let data = "🌱".repeat(2049);
        let token = capture(temp.path(), "text/plain;charset=utf-8", data.as_bytes()).unwrap();
        let (dir, items) = contents(temp.path(), &token).unwrap();
        assert_eq!(items[0]["@type"], "inputMessageDocument");
        assert_eq!(
            fs::read(dir.join("0/clipboard.txt")).unwrap(),
            data.as_bytes()
        );
    }
    #[test]
    fn unsupported_empty_remote_directory_and_fifo_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let fifo = temp.path().join("fifo");
        let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        for (types, data) in [
            ("text/plain", " \n".to_string()),
            ("application/octet-stream", "".into()),
            ("text/uri-list", uri(temp.path()).unwrap()),
            ("text/uri-list", "file://remote/private".into()),
            ("text/uri-list", uri(&fifo).unwrap()),
            ("text/uri-list", "# no files\n".into()),
        ] {
            assert!(
                capture(temp.path(), types, data.as_bytes()).is_err(),
                "{types}"
            );
        }
    }
    #[test]
    fn count_and_total_size_are_enforced_at_capture_and_delivery() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("fixture.txt");
        fs::write(&path, b"synthetic").unwrap();
        let selection = vec![uri(&path).unwrap(); 11].join("\n");
        assert!(capture(temp.path(), "text/uri-list", selection.as_bytes()).is_err());
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(MAX_BYTES / 2 + 1).unwrap();
        let selection = vec![uri(&path).unwrap(); 2].join("\n");
        assert!(capture(temp.path(), "text/uri-list", selection.as_bytes()).is_err());
        file.set_len(MAX_BYTES + 1).unwrap();
        assert!(capture(temp.path(), "text/uri-list", uri(&path).unwrap().as_bytes()).is_err());
        let token = capture(temp.path(), "image/png", b"synthetic").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(temp.path().join(&token).join("0/clipboard.png"))
            .unwrap()
            .set_len(MAX_BYTES + 1)
            .unwrap();
        assert!(contents(temp.path(), &token).is_err());
    }
    #[test]
    fn gnome_files_are_independent_snapshots() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("space 中文.txt");
        fs::write(&source, b"synthetic").unwrap();
        let data = format!("copy\n{}\n", uri(&source).unwrap());
        let token = capture(temp.path(), "x-special/gnome-copied-files", data.as_bytes()).unwrap();
        fs::remove_file(source).unwrap();
        let (_, items) = contents(temp.path(), &token).unwrap();
        let path = items[0]["document"]["document"]["path"].as_str().unwrap();
        assert_eq!(fs::read(path).unwrap(), b"synthetic");
        assert!(path.ends_with("space 中文.txt"));
    }
    #[test]
    fn attachment_escape_and_bad_tokens_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        for token in [
            "",
            "..",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "0123/56789abcdef0123456789abcdef",
        ] {
            assert!(contents(temp.path(), token).is_err());
        }
        let outside = temp.path().join("outside.txt");
        fs::write(&outside, b"synthetic").unwrap();
        let token = capture(temp.path(), "text/plain", b"synthetic").unwrap();
        let directory = temp.path().join(&token);
        fs::write(
            directory.join("items.json"),
            br#"[{"file":"../outside.txt"}]"#,
        )
        .unwrap();
        assert!(contents(temp.path(), &token).is_err());
        symlink(&outside, directory.join("link")).unwrap();
        fs::write(directory.join("items.json"), br#"[{"file":"link"}]"#).unwrap();
        assert!(contents(temp.path(), &token).is_err());
    }
    #[test]
    fn offline_cli_never_reads_clipboard() {
        let temp = tempfile::tempdir().unwrap();
        let mut notices = vec![];
        assert!(save_with(
            temp.path(),
            |_| Ok(json!({"authorized":false})),
            |_| panic!("must not read clipboard"),
            |n| notices.push(n)
        )
        .is_err());
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0]["timeout"], 5000);
    }
    #[test]
    fn uncertain_cli_retains_snapshot_but_rejection_removes_it() {
        let temp = tempfile::tempdir().unwrap();
        for uncertain in [true, false] {
            let token = capture(temp.path(), "text/plain", b"synthetic").unwrap();
            let mut notices = vec![];
            let result = save_with(
                temp.path(),
                |v| {
                    Ok(if v["action"] == "status" {
                        json!({"authorized":true,"connected":true})
                    } else {
                        json!({"ok":false,"uncertain":uncertain,"error":"private content must not be repeated"})
                    })
                },
                |_| Ok(token.clone()),
                |n| notices.push(n),
            );
            assert!(result.is_err());
            assert_eq!(temp.path().join(token).exists(), uncertain);
            assert!(!notices[0].to_string().contains("private content"));
        }
    }
}
