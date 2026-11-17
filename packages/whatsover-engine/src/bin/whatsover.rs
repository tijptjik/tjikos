#[path = "support/history.rs"]
mod history;
// Direct desktop host. Uses its own linked device, never the Cloudflare service.
#[path = "support/contact_input.rs"]
mod contact_input;
#[path = "support/delivery.rs"]
mod delivery;
#[path = "support/image_input.rs"]
mod image_input;
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::{io::Read, path::PathBuf, time::Duration};
use wacore::store::{DevicePropsOverride, traits::DeviceStore};
use whatsapp_rust::prelude::Bot;
use whatsover_engine::{Engine, Outbound, SendState, TextRequest, native::NativeAccount};

#[derive(Parser)]
#[command(
    name = "whatsover",
    version,
    about = "Whatsover — direct WhatsApp client; session data stays outside the repository"
)]
struct Args {
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    /// Deadline for login/check/send work (shutdown drains separately).
    #[arg(long, global = true, default_value_t = 180, value_parser = clap::value_parser!(u64).range(1..))]
    timeout_seconds: u64,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Pair via terminal QR, persist the session, then exit.
    Login,
    /// Show whether local credentials exist, without connecting.
    Status,
    /// Verify the linked session can connect; sends no chat message.
    Check,
    /// Save first/last names to WhatsApp (one JSON object or an array on stdin).
    Contact {
        #[arg(long, required = true)]
        stdin: bool,
    },
    /// Pair via terminal QR, then receive messages until Ctrl+C.
    Run,
    /// Collect server/delivery receipts without sending a new chat message.
    Observe {
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..))]
        seconds: u64,
    },
    /// Request one bounded page of older messages from the primary phone.
    History {
        #[arg(long)]
        to: String,
        /// Oldest known cursor message; omit for a best-effort timestamp-only request.
        #[arg(long, default_value = "")]
        before_id: String,
        #[arg(long)]
        before_from_me: bool,
        /// Cursor Unix timestamp in milliseconds; defaults to now.
        #[arg(long, value_parser = clap::value_parser!(i64).range(1..))]
        before_timestamp_ms: Option<i64>,
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(i32).range(1..=50))]
        count: i32,
    },
    /// Download media from a saved private inbound payload; never sends chat messages.
    MediaDownload {
        #[arg(long)]
        payload: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Send once with a durable idempotency key; repeats return the saved result.
    Send {
        #[arg(long)]
        to: String,
        #[arg(long, required_unless_present_any = ["stdin", "image"], conflicts_with = "stdin")]
        text: Option<String>,
        /// Send an image as this message, with optional --text/--stdin caption.
        #[arg(long)]
        image: Option<PathBuf>,
        /// Read UTF-8 message text from stdin instead of process arguments.
        #[arg(long)]
        stdin: bool,
        #[arg(long, alias = "id")]
        request_id: String,
        /// Also require a matching recipient delivery/read receipt before success.
        #[arg(long)]
        wait_delivery: bool,
    },
    /// Read the persisted inbox without connecting to WhatsApp.
    Inbox {
        #[arg(long, default_value_t = 0)]
        after: i64,
        #[arg(long, default_value_t = 100)]
        limit: i64,
    },
    /// Inspect a history request and captured-page reports offline.
    HistoryResult { request_id: String },
    /// Inspect an earlier send without connecting to WhatsApp.
    Result { request_id: String },
}

fn default_state_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("WHATSOVER_RUST_STATE_DIR") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path).join("whatsover-rust"));
    }
    Ok(
        PathBuf::from(std::env::var_os("HOME").context("set --state-dir or HOME")?)
            .join(".local/state/whatsover-rust"),
    )
}

fn message_text(text: &Option<String>, stdin: bool, input: impl Read) -> Result<String> {
    if !stdin {
        return text.clone().context("provide --text or --stdin");
    }
    let mut bytes = Vec::new();
    input.take(16_385).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 16_384, "text must be 1–16384 bytes");
    String::from_utf8(bytes).context("stdin must be UTF-8")
}

async fn print_send_result(
    journal: &whatsover_engine::native::SqliteJournal,
    result: &Outbound,
    wait_delivery: bool,
) -> Result<()> {
    let value = delivery::result_json(journal, result).await?;
    println!("{}", value);
    anyhow::ensure!(
        result.state == SendState::Submitted,
        "send outcome is not submitted; inspect this request ID and the chat before deciding to resend"
    );
    anyhow::ensure!(
        value["transport"]["server"] == "accepted",
        "server acceptance is not confirmed; inspect this request ID before any resend"
    );
    anyhow::ensure!(
        !wait_delivery || value["transport"]["recipientDeliveryConfirmed"] == true,
        "recipient delivery is not confirmed; do not blindly resend"
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let wait_delivery = matches!(
        args.command,
        Command::Send {
            wait_delivery: true,
            ..
        }
    );
    let mut image = match &args.command {
        Command::Send {
            image: Some(path), ..
        } => Some(image_input::read(path)?),
        _ => None,
    };
    // Validate the complete batch before opening a session or writing any name.
    let contacts = if matches!(args.command, Command::Contact { .. }) {
        Some(contact_input::read(std::io::stdin().lock())?)
    } else {
        None
    };
    let request = match &args.command {
        Command::Send {
            to,
            text,
            stdin,
            request_id,
            ..
        } => Some(
            TextRequest {
                to: to.clone(),
                text: if image.is_some() && text.is_none() && !stdin {
                    String::new()
                } else {
                    message_text(text, *stdin, std::io::stdin().lock())?
                },
                request_id: request_id.clone(),
                reply_to: None,
                reaction: None,
                attachment_id: image.as_ref().map(|image| image.id.clone()),
            }
            .validated()?,
        ),
        _ => None,
    };
    let media_input = if let Command::MediaDownload { payload, output } = &args.command {
        anyhow::ensure!(!output.exists(), "output already exists");
        let bytes = std::fs::read(payload)?;
        anyhow::ensure!(bytes.len() <= 2 * 1024 * 1024, "payload too large");
        Some(serde_json::from_slice::<whatsover_engine::content::StoredInbound>(&bytes)?)
    } else { None };
    let directory = match args.state_dir {
        Some(path) => path,
        None => default_state_dir()?,
    };
    let account = NativeAccount::open(&directory).await?;
    match args.command {
        Command::Status => {
            let paired = account
                .protocol
                .load()
                .await?
                .is_some_and(|d| d.pn.is_some());
            println!("{}", serde_json::json!({"paired": paired}));
            return Ok(());
        }
        Command::Inbox { after, limit } => {
            for row in account.journal.inbox(after, limit).await? {
                println!("{}", serde_json::to_string(&row)?);
            }
            return Ok(());
        }
        Command::HistoryResult { request_id } => {
            println!("{}", account.journal.history_result(&request_id).await?);
            return Ok(());
        }
        Command::Result { request_id } => {
            let value = match account.journal.outbound(&request_id).await? {
                Some(result) => delivery::result_json(&account.journal, &result).await?,
                None => serde_json::Value::Null,
            };
            println!("{}", value);
            return Ok(());
        }
        _ => {}
    }
    // Already recorded requests can be inspected/replayed even when offline.
    if let Some(request) = &request {
        if let Some(result) = account.journal.outbound(&request.request_id).await? {
            anyhow::ensure!(
                result.request == *request,
                "requestId already belongs to a different message"
            );
            return print_send_result(&account.journal, &result, wait_delivery).await;
        }
    }
    let pairing = matches!(args.command, Command::Login | Command::Run);
    if !pairing {
        anyhow::ensure!(
            account
                .protocol
                .load()
                .await?
                .is_some_and(|d| d.pn.is_some()),
            "no linked Rust session; run `whatsover login` in a terminal first"
        );
    }
    let history = history::History::new(account.journal.clone());
    let history_target = if let Command::History { to, .. } = &args.command {
        Some(whatsover_engine::normalize_recipient(to)?)
    } else {
        None
    };
    let mut history_request_id: Option<String> = None;
    let mut history_reports = Vec::new();
    let delivery_events = delivery::DeliveryEvents::default();
    let engine = Engine::start(
        Bot::builder()
            .with_backend(account.protocol.clone())
            .with_history_sync_durability_hook(history.clone())
            .with_event_handler(history.clone())
            .with_event_handler(delivery_events.clone())
            .with_device_props(
                DevicePropsOverride::new()
                    .with_os("Whatsover")
                    .with_platform_type(waproto::whatsapp::device_props::PlatformType::DESKTOP),
            ),
        account.journal.clone(),
    )
    .await?;
    let mut send_printed = false;
    let work = async {
        let mut previous_qr = None;
        let mut previous_connection = false;
        let mut observation_started = None;
        loop {
            delivery_events.flush(&account.journal).await?;
            history_reports.extend(history.flush().await?);
            let status = engine.status.snapshot();
            if status.qr != previous_qr {
                if let Some(qr) = &status.qr {
                    anyhow::ensure!(pairing, "session needs pairing; run `whatsover login`");
                    eprintln!(
                        "Scan with WhatsApp → Linked devices:\n{}",
                        qrcode::QrCode::new(qr)?
                            .render::<qrcode::render::unicode::Dense1x2>()
                            .quiet_zone(true)
                            .build()
                    );
                }
                previous_qr = status.qr;
            }
            if status.connected != previous_connection {
                eprintln!("{}", serde_json::json!({"connected": status.connected}));
                previous_connection = status.connected;
            }
            if status.logged_out {
                bail!("WhatsApp logged out this linked device");
            }
            if let Command::History {
                before_id,
                before_from_me,
                before_timestamp_ms,
                count,
                ..
            } = &args.command
            {
                if status.connected && history_request_id.is_none() {
                    engine
                        .client
                        .wait_for_startup_sync(Duration::from_secs(args.timeout_seconds))
                        .await?;
                    let id = engine.client.generate_message_id();
                    let now = wacore::time::now_millis();
                    let cursor = before_timestamp_ms.unwrap_or(now);
                    account.journal.reserve_history_request(id.as_str(), &serde_json::json!({"requestedAtMs":now,"to":history_target,"beforeId":before_id,"beforeFromMe":before_from_me,"beforeTimestampMs":cursor,"count":count})).await?;
                    history_request_id = Some(id.to_string());
                    engine
                        .client
                        .fetch_message_history_with_id(
                            &history_target.as_ref().unwrap().parse()?,
                            before_id,
                            *before_from_me,
                            cursor,
                            *count,
                            &id,
                        )
                        .await?;
                    eprintln!(
                        "History request sent to primary phone; waiting for matching history response"
                    );
                }
                if history_request_id.as_ref().is_some_and(|id| {
                    history_reports
                        .iter()
                        .any(|r| r["requestId"].as_str() == Some(id.as_str()))
                }) {
                    println!(
                        "{}",
                        serde_json::json!({"requestId":history_request_id,"chunks":history_reports,"historyComplete":false})
                    );
                    return Ok(());
                }
            } else if let Command::MediaDownload { output, .. } = &args.command {
                if status.connected {
                    let bytes = whatsover_engine::media::download(&engine.client, &media_input.as_ref().unwrap().message).await?;
                    use std::io::Write;
                    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(output)?;
                    file.write_all(&bytes)?;
                    file.sync_all()?;
                    println!("{}", serde_json::json!({"downloadedBytes":bytes.len(),"output":output,"newChatMessagesSent":0}));
                    return Ok(());
                }
            } else if let Some(request) = &request {
                if status.connected {
                    engine
                        .client
                        .wait_for_session_ready(Duration::from_secs(args.timeout_seconds))
                        .await?;
                    engine
                        .client
                        .wait_for_startup_sync(Duration::from_secs(args.timeout_seconds))
                        .await?;
                    if let Some(image) = image.take() {
                        let payload = whatsover_engine::media::prepare(
                            &engine.client,
                            image.upload,
                            image.bytes,
                        )
                        .await?;
                        account
                            .journal
                            .stage_attachment(image.id, serde_json::to_string(&payload)?)
                            .await?;
                    }
                    let result = engine.send(request.clone()).await?;
                    // The SDK result only proves that the send call completed.
                    // Keep the connection alive for its matching server ACK/NACK.
                    loop {
                        delivery_events.flush(&account.journal).await?;
                        let value = delivery::result_json(&account.journal, &result).await?;
                        let server = value["transport"]["server"]
                            .as_str()
                            .unwrap_or("unconfirmed");
                        if result.state != SendState::Submitted
                            || server == "rejected"
                            || (server == "accepted"
                                && (!wait_delivery
                                    || value["transport"]["recipientDeliveryConfirmed"] == true))
                        {
                            send_printed = true;
                            return print_send_result(&account.journal, &result, wait_delivery)
                                .await;
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            } else if let Some(contacts) = &contacts {
                if status.connected {
                    for contact in contacts {
                        let full_name = [&contact.first_name, &contact.last_name]
                            .into_iter()
                            .filter(|s| !s.is_empty())
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(" ");
                        engine
                            .client
                            .chat_actions()
                            .save_contact(
                                &contact.to.parse()?,
                                Some(full_name.clone()),
                                Some(contact.first_name.clone()),
                                true,
                            )
                            .await?;
                        println!(
                            "{}",
                            serde_json::json!({"to": contact.to, "firstName": contact.first_name, "lastName": contact.last_name, "fullName": full_name, "state": "saved", "primaryAddressbookSyncRequested": true})
                        );
                    }
                    return Ok(());
                }
            } else if let Command::Observe { seconds } = args.command {
                if status.connected {
                    let start = observation_started.get_or_insert_with(tokio::time::Instant::now);
                    if start.elapsed() >= Duration::from_secs(seconds) {
                        println!(
                            "{}",
                            serde_json::json!({"observedSeconds":seconds,"newChatMessagesSent":0})
                        );
                        return Ok(());
                    }
                }
            } else if status.connected && !matches!(args.command, Command::Run) {
                println!("{}", serde_json::json!({"connected": true, "paired": true}));
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    };
    let outcome = tokio::select! {
        result = work => result,
        _ = tokio::time::sleep(Duration::from_secs(args.timeout_seconds)), if !matches!(args.command, Command::Run) => {
            if let Some(id) = &history_request_id {
                Err(anyhow::anyhow!("history request timed out; inspect `whatsover history-result {id}`; no automatic retry was made"))
            } else {
                Err(anyhow::anyhow!("operation timed out; inspect `result <request-id>` before retrying any send"))
            }
        },
        result = tokio::signal::ctrl_c() => {
            result?;
            if matches!(args.command, Command::Run) { Ok(()) }
            else { Err(anyhow::anyhow!("interrupted; inspect the request result before retrying any send")) }
        },
    };
    let report = engine.shutdown().await?;
    delivery_events.flush(&account.journal).await?;
    history.flush().await?;
    if outcome.is_err() && !send_printed {
        if let Some(request) = &request {
            if let Some(result) = account.journal.outbound(&request.request_id).await? {
                println!(
                    "{}",
                    delivery::result_json(&account.journal, &result).await?
                );
            }
        }
    }
    report
        .shutdown
        .device
        .context("device state did not flush during shutdown")?;
    anyhow::ensure!(
        report.shutdown.message_secrets.failed_batches == 0,
        "message-secret persistence failed during this session"
    );
    for drain in [
        report.shutdown.inbound,
        report.shutdown.outbound,
        report.shutdown.signal_settle,
    ] {
        anyhow::ensure!(
            drain == whatsapp_rust::prelude::DrainOutcome::Completed,
            "session shutdown did not finish draining: {drain:?}"
        );
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdin_preserves_message_and_rejects_oversize_or_invalid_utf8() {
        assert_eq!(
            message_text(&None, true, "Hello\n世界\n".as_bytes()).unwrap(),
            "Hello\n世界\n"
        );
        assert!(message_text(&None, true, vec![b'x'; 16_385].as_slice()).is_err());
        assert!(message_text(&None, true, [0xff].as_slice()).is_err());
    }

    #[test]
    fn send_requires_exactly_one_text_source_and_positive_timeout() {
        let prefix = ["whatsover", "send", "--to", "+15555550100", "--id", "job-1"];
        assert!(Args::try_parse_from(prefix).is_err());
        assert!(Args::try_parse_from([prefix.as_slice(), &["--stdin"]].concat()).is_ok());
        assert!(
            Args::try_parse_from([prefix.as_slice(), &["--stdin", "--text", "hello"]].concat())
                .is_err()
        );
        assert!(Args::try_parse_from(["whatsover", "check", "--timeout-seconds", "0"]).is_err());
    }
}
