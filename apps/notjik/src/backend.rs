//! Renderer-independent configuration, history, sync, and lock-state runtime.
//!
//! JSON messages share one contract across in-process and diagnostic transports.
//! This does not yet own the notification queue or the desktop D-Bus name.

/// One bounded input message, including a transport decoding error.
pub type Request = std::result::Result<serde_json::Value, String>;

/// A frontend adapter supplies messages and receives service updates.
/// `receive` must honor its timeout so lock/config polling cannot be starved.
/// Disconnection ends the runtime; `publish` errors are fatal, not silently lost.
pub trait Transport {
    fn receive(
        &mut self,
        timeout: std::time::Duration,
    ) -> std::result::Result<Request, std::sync::mpsc::RecvTimeoutError>;
    fn publish(&mut self, event: &serde_json::Value) -> anyhow::Result<()>;
}

use crate::{config, history, lock::Observer, util::*};
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};
pub fn dismiss_request(cfg: &Value, e: &Value) -> Result<Option<Value>> {
    let n = &e["notice"];
    let d = &n["data"];
    let explicit = e["event"] == "user-safe-read";
    if (e["event"] != "user-dismissed" && !explicit)
        || n["type"] != "telegram"
        || n["app"] != "Telegram via Notjik"
        || truthy(&d["demo"])
        || truthy(&n["archived"])
        || (!explicit
            && cfg["telegram"]["dismissIsRead"]
                .as_array()
                .is_none_or(|a| a.is_empty()))
    {
        return Ok(None);
    }
    let chat = crate::telegram::identifier(&d["chatId"])?;
    let message = crate::telegram::identifier(&d["messageId"])?;
    Ok(Some(
        json!({"action":if explicit{"safeRead"}else{"dismissRead"},"chatId":chat.to_string(),"messageId":message.to_string()}),
    ))
}
/// Run native services on a dedicated worker thread until the transport closes.
/// The caller owns process policy (including umask) and transport lifetime.
/// No GTK objects may be accessed by a transport on this thread.
pub fn run(root: &Path, transport: &mut impl Transport) -> Result<()> {
    transport.publish(&json!({"paused":history::paused()?}))?;
    let sync_enabled = notjik_sync::client::configured()
        && (std::env::var_os("NOTJIK_STATE_DIR").is_none()
            || std::env::var_os("NOTJIK_SYNC_CONFIG").is_some());
    transport.publish(&json!({"syncEnabled":sync_enabled}))?;
    let (dismiss_tx, dismiss_rx) = mpsc::sync_channel::<Value>(100);
    let (result_tx, results) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(r) = dismiss_rx.recv() {
            let result = crate::telegram::socket_request(&r).and_then(|v| {
                ensure!(
                    v["ok"] == true,
                    "{}",
                    v["error"].as_str().unwrap_or("Telegram rejected request")
                );
                Ok(())
            });
            if let Err(e) = result {
                let _ = result_tx.send(e.to_string());
            }
        }
    });
    let mut cfg = None;
    let mut db = None;
    let mut previous = String::new();
    let mut observer = Observer::new();
    let mut lock = "";
    let mut idle_seconds = u64::MAX;
    let mut dirty = true;
    let mut next_config = Instant::now();
    let mut next_prune = Instant::now();
    let mut next_history = Instant::now();
    loop {
        observer.poll();
        let idle = observer.idle_seconds();
        if idle != idle_seconds {
            idle_seconds = idle;
            transport.publish(&json!({"idle":idle >= 120,"idleSeconds":idle}))?;
        }
        if lock != observer.state {
            lock = observer.state;
            transport.publish(&json!({"lockState":lock}))?;
        }
        for error in results.try_iter() {
            transport.publish(&json!({"error":format!("Dismiss read: {error}")}))?;
        }
        let now = Instant::now();
        if now >= next_config {
            next_config = now + Duration::from_secs(1);
            match config::load(root) {
                Ok(c) => {
                    let serial = c.to_string();
                    if serial != previous {
                        transport.publish(&json!({"config":c}))?;
                        cfg = Some(c);
                        previous = serial;
                        dirty = true;
                        next_prune = now;
                    }
                }
                Err(e) => {
                    let message = format!("Configuration: {e}");
                    if message != previous {
                        transport.publish(&json!({"error":message}))?;
                        previous = message;
                    }
                }
            }
        }
        if let Some(c) = &cfg {
            let result = (|| -> Result<()> {
                if db.is_none() {
                    db = Some(history::database()?);
                }
                let db = db.as_ref().unwrap();
                if now >= next_prune {
                    next_prune = now + Duration::from_secs(60);
                    dirty |= history::prune(db, &c["history"])?;
                }
                if dirty && now >= next_history {
                    next_history = now + Duration::from_millis(200);
                    transport.publish(
                        &json!({"avatars":history::avatars(db)?,"history":history::notices(db,c["history"]["drawerLimit"].as_u64().unwrap_or(100) as usize)?}),
                    )?;
                    dirty = false;
                }
                Ok(())
            })();
            if let Err(e) = result {
                transport.publish(&json!({"error":format!("History: {e}")}))?;
            }
        }
        match transport.receive(Duration::from_millis(100)) {
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(input) => {
                let result = (|| -> Result<()> {
                    let e = input.map_err(anyhow::Error::msg)?;
                    ensure!(e.is_object(), "Worker event must be an object");
                    if e["event"] == "dismissed"
                        && e["notice"]["globalId"]
                            .as_str()
                            .is_some_and(|s| !s.is_empty())
                        && sync_enabled
                    {
                        // Durable enqueue only; network work belongs to the sync service.
                        notjik_sync::client::enqueue(
                            json!({"op":"dismiss","id":e["notice"]["globalId"]}),
                        )?;
                    }
                    if e["op"] == "pausePreference" {
                        history::save_paused(&e["paused"])?;
                    } else if e["op"] == "history" {
                        dirty = true;
                    } else if let Some(c) = &cfg {
                        if let Some(r) = dismiss_request(c, &e)? {
                            if dismiss_tx.try_send(r).is_err() {
                                transport.publish(
                                    &json!({"error":"Dismiss read: queue full; message left unread"}),
                                )?;
                            }
                        }
                        if e["event"] != "user-dismissed" && e["event"] != "user-safe-read" {
                            if db.is_none() {
                                db = Some(history::database()?);
                            }
                            history::record(db.as_mut().unwrap(), c, &e)?;
                            dirty = true;
                        }
                    }
                    Ok(())
                })();
                if let Err(e) = result {
                    transport.publish(&json!({"error":format!("History: {e}")}))?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dismiss_never_reads_demo_or_archived_cards() {
        let mut cfg = crate::config::defaults();
        cfg["telegram"]["dismissIsRead"] = json!(["123"]);
        let event = json!({"event":"user-safe-read","notice":{"app":"Telegram via Notjik","type":"telegram","data":{"chatId":"123","messageId":"456"}}});
        assert!(dismiss_request(&cfg, &event).unwrap().is_some());
        for flag in [json!(true), json!("synthetic"), json!({"fixture":true})] {
            let mut e = event.clone();
            e["notice"]["data"]["demo"] = flag.clone();
            assert!(dismiss_request(&cfg, &e).unwrap().is_none());
            let mut e = event.clone();
            e["notice"]["archived"] = flag;
            assert!(dismiss_request(&cfg, &e).unwrap().is_none());
        }
    }
}
