//! Unattended workflow entry point with a durable, pinned Telegram status.
use crate::{actions::State, scheduler::Receipt};
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::Path,
    time::Duration,
};

#[derive(Deserialize)]
struct Destination {
    bot_token: String,
    chat_id: Value,
    topic_id: Option<i64>,
}

fn save(path: &Path, value: &Value) -> Result<()> {
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().context("missing state directory")?)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

fn summary(host: &str, code: i32, receipt: Option<&Receipt>) -> String {
    let attention = receipt.is_some_and(|r| {
        r.recipes
            .values()
            .any(|o| matches!(o.state, State::Stopped | State::Blocked) || !o.warnings.is_empty())
    });
    let status = if code != 0 {
        "FAILED"
    } else if attention {
        "ATTENTION"
    } else {
        "SUCCESS"
    };
    let mut text = format!(
        "Tjikup · {host}\n{status} · {}\n",
        chrono::Utc::now().to_rfc3339()
    );
    if let Some(r) = receipt {
        let completed = r
            .recipes
            .values()
            .filter(|o| matches!(o.state, State::Done | State::Unchanged))
            .count();
        text.push_str(&format!(
            "{completed}/{} stages completed · {:.0}s\n",
            r.recipes.len(),
            r.seconds
        ));
        // Recipe IDs only: errors, warnings and logs can contain private command output.
        for (id, o) in r
            .recipes
            .iter()
            .filter(|(_, o)| {
                matches!(o.state, State::Failed | State::Stopped | State::Blocked)
                    || !o.warnings.is_empty()
            })
            .take(8)
        {
            text.push_str(&format!(
                "{id}: {:?}{}\n",
                o.state,
                if o.warnings.is_empty() {
                    ""
                } else {
                    " (warnings)"
                }
            ));
        }
    } else {
        text.push_str("Workflow did not produce a completion receipt.\n");
    }
    text.push_str("Logs: journalctl --user -u tjikup-weekly.service");
    text.chars().take(3500).collect()
}

fn api(
    client: &reqwest::blocking::Client,
    base: &str,
    destination: &Destination,
    method: &str,
    payload: Value,
) -> Result<Value> {
    // Never expose reqwest errors or the response body: the request URL contains a token.
    let response = client
        .post(format!("{base}/bot{}/{method}", destination.bot_token))
        .json(&payload)
        .send()
        .map_err(|_| anyhow::anyhow!("Telegram transport failed"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .map_err(|_| anyhow::anyhow!("Invalid Telegram response"))?;
    // A retry after an acknowledged edit can encounter already-current server state.
    if method == "editMessageText"
        && status.as_u16() == 400
        && body["description"]
            .as_str()
            .is_some_and(|s| s.starts_with("Bad Request: message is not modified"))
    {
        return Ok(Value::Bool(true));
    }
    ensure!(
        status.is_success(),
        "Telegram {method} failed (HTTP {})",
        status.as_u16()
    );
    ensure!(body["ok"] == true, "Telegram rejected {method}");
    Ok(body["result"].clone())
}

fn deliver(destination: &Destination, state: &Path, text: &str, base: &str) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| anyhow::anyhow!("Notification transport unavailable"))?;
    let binding = json!({"chat_id": destination.chat_id, "topic_id": destination.topic_id,
        "bot_id": destination.bot_token.split(':').next()});
    let previous: Value = fs::read(state)
        .ok()
        .and_then(|s| serde_json::from_slice(&s).ok())
        .unwrap_or(Value::Null);
    let id = if previous["destination"] == binding && previous["message_id"].is_i64() {
        let id = previous["message_id"].clone();
        if previous["text"] != text {
            api(
                &client,
                base,
                destination,
                "editMessageText",
                json!({"chat_id":destination.chat_id,"message_id":id,"text":text}),
            )?;
        }
        id
    } else {
        let mut payload = json!({"chat_id":destination.chat_id,"text":text});
        if let Some(topic) = destination.topic_id.filter(|id| *id != 0) {
            payload["message_thread_id"] = json!(topic);
        }
        let message = api(&client, base, destination, "sendMessage", payload)?;
        ensure!(
            message["message_id"].is_i64(),
            "Telegram returned no message ID"
        );
        message["message_id"].clone()
    };
    // Record the sent ID before pinning so a pin failure cannot produce duplicates.
    save(
        state,
        &json!({"destination":binding,"message_id":id,"text":text}),
    )?;
    api(
        &client,
        base,
        destination,
        "pinChatMessage",
        json!({"chat_id":destination.chat_id,"message_id":id,"disable_notification":true}),
    )?;
    Ok(())
}

fn destination() -> Result<Destination> {
    let credentials = std::env::var_os("TJIKUP_TELEGRAM_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| crate::commands::home().unwrap_or_default().join(".config"))
                .join("tjikup/telegram.json")
        });
    let metadata = fs::metadata(&credentials).context("Telegram credentials unavailable")?;
    ensure!(
        metadata.mode() & 0o077 == 0,
        "Telegram credentials must be private (0600)"
    );
    let config: Value = serde_json::from_slice(&fs::read(credentials)?)
        .context("Invalid Telegram configuration")?;
    let destination = resolve_destination(config)?;
    ensure!(
        !destination.bot_token.is_empty()
            && (destination.chat_id.is_string() || destination.chat_id.is_i64()),
        "Missing Telegram token or chat ID"
    );
    Ok(destination)
}

fn resolve_destination(config: Value) -> Result<Destination> {
    if let Some(id) = config.get("bws_secret_id") {
        let id = id.as_str().context("Invalid Telegram BWS secret ID")?;
        let provider: crate::managed_files::secrets::Bitwarden = serde_json::from_value(
            config
                .get("bitwarden")
                .context("Missing Telegram BWS provider")?
                .clone(),
        )
        .context("Invalid Telegram BWS provider")?;
        let secrets = crate::managed_files::secrets::Secrets::new(provider)?;
        let value = secrets.get(id)?;
        serde_json::from_str(&value)
            .map_err(|_| anyhow::anyhow!("BWS Telegram secret must contain destination JSON"))
    } else {
        serde_json::from_value(config).map_err(|_| anyhow::anyhow!("Invalid Telegram credentials"))
    }
}

fn relay_report(argv: &[String], text: &str) -> Result<()> {
    use std::process::{Command, Stdio};
    let host = fs::read_to_string("/proc/sys/kernel/hostname")?;
    let mut payload = tempfile::tempfile()?;
    serde_json::to_writer(&mut payload, &json!({"host":host.trim(), "text":text}))?;
    use std::io::Seek;
    payload.rewind()?;
    let result = crate::command_io::capture_input(
        Command::new(&argv[0]).args(&argv[1..]),
        Duration::from_secs(90),
        Stdio::from(payload),
    )?;
    if result.code != 0 {
        eprintln!("Telegram relay: {}", result.stderr);
    }
    ensure!(
        result.code == 0,
        "Telegram relay failed; inspect the server journal"
    );
    Ok(())
}

pub fn run(args: Vec<String>) -> Result<i32> {
    if args == ["--help"] {
        println!("tjikup scheduled [workflow flags]\nRuns the normal workflow and updates a pinned Telegram status.\nCredentials: TJIKUP_TELEGRAM_CONFIG or ~/.config/tjikup/telegram.json\nUse --notify-only to retry delivery without rerunning maintenance.");
        return Ok(0);
    }
    let root = crate::scheduler::state_root().join("scheduled");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&root)?;
    if args == ["--report-stdin"] {
        let destination = destination()?;
        let mut input = Vec::new();
        std::io::stdin().take(16385).read_to_end(&mut input)?;
        ensure!(input.len() <= 16384, "Status payload too large");
        let report: Value = serde_json::from_slice(&input)?;
        let host = report["host"].as_str().context("Missing report host")?;
        ensure!(
            !host.is_empty()
                && host.len() <= 63
                && host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "Invalid report host"
        );
        let _lock = crate::process::Lock::acquire(&root.join(format!("report-{host}.lock")))?;
        let text = report["text"].as_str().context("Missing report text")?;
        ensure!(text.chars().count() <= 3500, "Status text too large");
        deliver(
            &destination,
            &root.join(format!("message-{host}.json")),
            text,
            "https://api.telegram.org",
        )?;
        return Ok(0);
    }
    let _lock = crate::process::Lock::acquire(&root.join("run.lock"))?;
    let relay_path = std::env::var_os("TJIKUP_TELEGRAM_RELAY").map(std::path::PathBuf::from);
    let relay: Option<Vec<String>> = relay_path
        .map(|path| -> Result<_> {
            let argv: Vec<String> = serde_json::from_slice(&fs::read(path)?)?;
            ensure!(!argv.is_empty(), "Empty Telegram relay command");
            Ok(argv)
        })
        .transpose()?;
    let direct = if relay.is_none() {
        Some(destination()?)
    } else {
        None
    };
    let pending = root.join("latest.json");
    let (text, code) = if args == ["--notify-test"] {
        let host = fs::read_to_string("/proc/sys/kernel/hostname")?;
        (format!("Tjikup · {}\nWeekly status delivery verified.\nScheduled: Sunday 09:00 Asia/Hong_Kong.\nThis is a notification test; no maintenance run was performed.", host.trim()), 0)
    } else if args == ["--notify-only"] {
        let saved: Value = serde_json::from_slice(&fs::read(&pending)?)?;
        (
            saved["text"]
                .as_str()
                .context("Missing saved status")?
                .to_owned(),
            0,
        )
    } else {
        ensure!(
            args.first().is_none_or(|s| s.starts_with("--"))
                && !args.iter().any(|s| matches!(
                    s.as_str(),
                    "--check" | "--plan" | "--dry-run" | "--demo" | "--version" | "--help"
                )),
            "scheduled expects workflow flags, not a subcommand or preview"
        );
        std::env::set_var("TJIKUP_NOTIFY", "0");
        std::env::set_var("GIT_TERMINAL_PROMPT", "0");
        let host =
            fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_else(|_| "unknown".into());
        save(
            &pending,
            &json!({"text":format!("Tjikup · {}\nINCOMPLETE · started {}\nNo completion recorded; inspect tjikup-weekly.service.",host.trim(),chrono::Utc::now().to_rfc3339()),"delivered":false}),
        )?;
        let mut receipt = None;
        let code = match crate::cli(args, &mut receipt) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("tjikup: {error:#}");
                1
            }
        };
        (summary(host.trim(), code, receipt.as_ref()), code)
    };
    save(
        &pending,
        &json!({"text":text,"exit_code":code,"delivered":false}),
    )?;
    let delivery = if let Some(argv) = relay {
        relay_report(&argv, &text)
    } else {
        deliver(
            direct.as_ref().context("Missing destination")?,
            &root.join("message.json"),
            &text,
            "https://api.telegram.org",
        )
    };
    if let Err(error) = delivery {
        eprintln!("Status saved locally; retry with tjikup scheduled --notify-only");
        bail!("{error}");
    }
    save(
        &pending,
        &json!({"text":text,"exit_code":code,"delivered":true}),
    )?;
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn incomplete_run_is_not_success() {
        assert!(summary("demo", 1, None).contains("FAILED"));
        assert!(summary("demo", 1, None).contains("did not produce"));
    }
    #[test]
    fn stopped_work_is_attention_even_with_zero_exit() {
        let mut recipes = BTreeMap::new();
        recipes.insert(
            "backup-lock".into(),
            crate::scheduler::Outcome {
                events: vec![],
                state: State::Stopped,
                git_commits: None,
                dirty_files: None,
                seconds: 0.0,
                started: 0.0,
                dependency_wait: 0.0,
                resource_wait: 0.0,
                worker_wait: 0.0,
                error: Some("private secret".into()),
                warnings: vec!["private warning".into()],
                log: "/private/log".into(),
            },
        );
        let receipt = Receipt {
            version: 2,
            title: "demo".into(),
            interrupted: false,
            seconds: 1.0,
            recipes,
        };
        let text = summary("demo", 0, Some(&receipt));
        assert!(text.contains("ATTENTION"));
        assert!(!text.contains("private"));
    }
    #[test]
    fn delivery_reuses_message_and_retries_failed_pin() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut methods = vec![];
            for index in 0..4 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = vec![];
                let mut byte = [0];
                while !bytes.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                }
                let header = String::from_utf8(bytes).unwrap();
                let length: usize = header
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::to_owned)
                    })
                    .unwrap()
                    .parse()
                    .unwrap();
                let mut body = vec![0; length];
                socket.read_exact(&mut body).unwrap();
                methods.push(
                    header
                        .lines()
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .nth(1)
                        .unwrap()
                        .rsplit('/')
                        .next()
                        .unwrap()
                        .to_owned(),
                );
                let result = if index == 1 {
                    r#"{"ok":false}"#
                } else if index == 0 {
                    r#"{"ok":true,"result":{"message_id":42}}"#
                } else if index == 2 {
                    r#"{"ok":false,"description":"Bad Request: message is not modified"}"#
                } else {
                    r#"{"ok":true,"result":true}"#
                };
                write!(
                    socket,
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    if index == 2 {
                        "400 Bad Request"
                    } else {
                        "200 OK"
                    },
                    result.len(),
                    result
                )
                .unwrap();
            }
            methods
        });
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("message.json");
        let destination = Destination {
            bot_token: "123:synthetic".into(),
            chat_id: json!(1),
            topic_id: None,
        };
        assert!(deliver(&destination, &state, "first", &base).is_err());
        deliver(&destination, &state, "second", &base).unwrap();
        assert_eq!(
            server.join().unwrap(),
            [
                "sendMessage",
                "pinChatMessage",
                "editMessageText",
                "pinChatMessage"
            ]
        );
        assert!(!fs::read_to_string(state).unwrap().contains("synthetic"));
    }
    #[test]
    fn relay_receives_structured_status_on_stdin() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("report.json");
        relay_report(
            &["tee".into(), path.to_string_lossy().into_owned()],
            "synthetic status",
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(value["text"], "synthetic status");
        assert!(value["host"].is_string());
    }
    #[test]
    fn bws_destination_uses_native_provider_without_exposing_value() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let token = dir.path().join("environment");
        fs::write(&token, "BWS_ACCESS_TOKEN=synthetic-auth\n").unwrap();
        fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
        let provider = dir.path().join("bws");
        fs::write(&provider, r#"#!/bin/sh
printf '%s' '{"id":"00000000-0000-0000-0000-000000000001","value":"{\"bot_token\":\"123:synthetic\",\"chat_id\":42}"}'
"#).unwrap();
        fs::set_permissions(&provider, fs::Permissions::from_mode(0o700)).unwrap();
        let config = json!({"bws_secret_id":"00000000-0000-0000-0000-000000000001", "bitwarden":{"command":[provider],"access_token_file":token}});
        let destination = resolve_destination(config).unwrap();
        assert_eq!(destination.chat_id, 42);
        assert_eq!(destination.bot_token, "123:synthetic");
    }
}
