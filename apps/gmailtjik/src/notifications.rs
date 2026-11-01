use crate::model::Message;
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[cfg(test)]
pub fn notice(account: &str, message: &Message) -> serde_json::Value {
    rule_notice(
        account,
        message,
        crate::rules::evaluate(&crate::rules::defaults(), message, &[]),
    )
    .unwrap()
}
pub fn rule_notice(
    account: &str,
    message: &Message,
    outcome: crate::rules::Outcome,
) -> Option<serde_json::Value> {
    if matches!(outcome, crate::rules::Outcome::Silent) {
        return None;
    }
    let id = format!(
        "gmailtjik:{:x}",
        Sha256::digest(format!("{account}:{}", message.id))
    );
    if let crate::rules::Outcome::AuthCode(code) = outcome {
        return Some(
            serde_json::json!({"app":"gmailtjik","id":id,"run":id,"title":"Authentication code", "body":message.header("From").chars().take(256).collect::<String>(),
            "type":"auth-code","data":{"code":code},"actions":[{"label":"Copy code","copy":code}],"timeout":30000,"group":"gmailtjik-auth"}),
        );
    }
    Some(serde_json::json!({
        "app": "gmailtjik", "id": id, "title": message.header("From").chars().take(256).collect::<String>(),
        "body": message.header("Subject").chars().take(512).collect::<String>(), "timeout": 8000, "group": "gmailtjik"
    }))
}
pub fn send(notice: serde_json::Value) -> Result<()> {
    if notjik_sync::client::configured() {
        notjik_sync::client::publish(notice, "active", false)?;
        return Ok(());
    }
    let mut child = Command::new("notjikctl")
        .arg("receive")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Cannot start notjikctl")?;
    let write = child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&notice)?);
    if let Err(error) = write {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.into());
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!("Notification delivery failed");
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Notification delivery timed out");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub fn deliver(
    store: &crate::store::Store,
    message: &Message,
    baseline: i64,
    transport: impl FnOnce(&Message) -> Result<()>,
) -> Result<()> {
    let receipt = format!("notified:{}", message.id);
    if message.internal_date.parse::<i64>().unwrap_or(0) <= baseline
        || !message.unread()
        || store.get::<bool>(&receipt)?.unwrap_or(false)
    {
        return Ok(());
    }
    transport(message)?;
    store.put(&receipt, &true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suppresses_existing_mail_and_retries_failed_delivery_only() {
        let store = crate::store::Store::open(None).unwrap();
        let mut message = Message {
            id: "synthetic-new".into(),
            internal_date: "101".into(),
            label_ids: vec!["INBOX".into(), "UNREAD".into()],
            ..Default::default()
        };
        deliver(&store, &message, 101, |_| {
            panic!("Startup mail must be silent")
        })
        .unwrap();
        assert!(deliver(&store, &message, 100, |_| bail!("Notjik unavailable")).is_err());
        let attempts = std::cell::Cell::new(0);
        for _ in 0..2 {
            deliver(&store, &message, 100, |_| {
                attempts.set(attempts.get() + 1);
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(attempts.get(), 1);
        message.id = "synthetic-read".into();
        message.label_ids.clear();
        deliver(&store, &message, 100, |_| {
            panic!("Read mail must be silent")
        })
        .unwrap();
    }
    #[test]
    fn auth_has_one_copy_card_and_no_regular_mail_card() {
        use crate::model::{Body, Header, Part};
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        let message = Message {
            id: "synthetic-code".into(),
            payload: Part {
                mime_type: "text/plain".into(),
                headers: vec![Header {
                    name: "Subject".into(),
                    value: "Your verification code".into(),
                }],
                body: Body {
                    data: URL_SAFE_NO_PAD.encode("Your verification code is 004281."),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let n = notice("synthetic@example.test", &message);
        assert_eq!(n["type"], "auth-code");
        assert_eq!(n["data"]["code"], "004281");
        assert_eq!(n["actions"].as_array().unwrap().len(), 1);
        assert_eq!(n["actions"][0]["copy"], "004281");
        assert_ne!(n["group"], "gmailtjik");
    }
}
