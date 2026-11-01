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
    let link = crate::links::MailLink {
        account: account.into(),
        message: message.id.clone(),
        thread: message.thread_id.clone(),
    };
    let open =
        serde_json::json!({"label":"Open email","command":["gmailtjik","--open",link.url()]});
    if let crate::rules::Outcome::AuthCode(code) = outcome {
        return Some(
            serde_json::json!({"app":"gmailtjik","id":id,"run":id,"title":"Authentication code", "body":message.header("From").chars().take(256).collect::<String>(),
            "type":"auth-code","data":{"code":code},"icon":"gmailtjik","actions":[{"label":"Copy code","copy":code},open],"timeout":30000,"group":"gmailtjik-auth"}),
        );
    }
    Some(serde_json::json!({
        "app": "gmailtjik", "icon":"gmailtjik", "actions":[open], "id": id, "title": message.header("From").chars().take(256).collect::<String>(),
        "body": message.header("Subject").chars().take(512).collect::<String>(), "timeout": 8000, "group": "gmailtjik"
    }))
}

/// Best-effort artwork: failure must never prevent mail notification delivery.
/// Cache both hits and misses for a day in the private account database.
pub fn sender_image(
    store: &crate::store::Store,
    message: &Message,
    notice: &mut serde_json::Value,
) {
    if let Ok(Some(image)) = resolve_sender_image(store, message) {
        notice["image"] = serde_json::json!(image);
    }
}
fn resolve_sender_image(store: &crate::store::Store, message: &Message) -> Result<Option<String>> {
    let addresses = crate::model::addresses(&message.header("From"))?;
    if addresses.len() != 1 {
        return Ok(None);
    }
    let email = addresses[0].1.to_lowercase();
    let key = format!("sender-art:{:x}", Sha256::digest(&email));
    let now = chrono::Utc::now().timestamp();
    if let Some((time, image)) = store.get::<(i64, Option<String>)>(&key)? {
        if now >= time && now - time < 86400 {
            return Ok(image);
        }
    }
    let contacts = store
        .get::<Vec<crate::model::Contact>>("contacts")?
        .unwrap_or_default();
    let photo = contacts
        .iter()
        .find(|c| c.email.eq_ignore_ascii_case(&email) && !c.photo.is_empty())
        .and_then(|c| crate::google::contact_photo(&c.photo).ok())
        .and_then(|bytes| compact_image(&bytes).ok());
    let image = photo.or_else(|| {
        let (_, domain) = email.rsplit_once('@')?;
        // A fixed HTTPS favicon service avoids fetching arbitrary sender-controlled hosts.
        // Only the domain is sent, never the address, message, or Google credentials.
        if !domain.contains('.')
            || !domain
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            return None;
        }
        let response = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() < 2
                    && attempt.url().scheme() == "https"
                    && attempt.url().host_str().is_some_and(|h| {
                        h == "www.google.com"
                            || h == "t0.gstatic.com"
                            || h == "t1.gstatic.com"
                            || h == "t2.gstatic.com"
                    })
                {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()
            .ok()?
            .get("https://www.google.com/s2/favicons")
            .query(&[("domain", domain), ("sz", "64")])
            .send()
            .ok()?
            .error_for_status()
            .ok()?;
        use std::io::Read;
        let mut bytes = Vec::new();
        response.take(2_000_001).read_to_end(&mut bytes).ok()?;
        compact_image(&bytes).ok()
    });
    store.put(&key, &(now, &image))?;
    Ok(image)
}
fn compact_image(bytes: &[u8]) -> Result<String> {
    use base64::Engine;
    anyhow::ensure!(bytes.len() <= 2_000_000, "Sender image is too large");
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode()?.thumbnail(64, 64);
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png)?;
    let uri = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png.into_inner())
    );
    anyhow::ensure!(uri.len() <= 32768, "Sender artwork is too large");
    Ok(uri)
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
    fn sender_art_is_bounded_png_and_missing_sender_uses_app_icon() {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(200, 100, image::Rgba([20, 40, 60, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let uri = compact_image(bytes.get_ref()).unwrap();
        assert!(uri.starts_with("data:image/png;base64,"));
        assert!(uri.len() <= 32768);
        assert!(compact_image(b"not an image").is_err());
        let message = Message {
            id: "abc123".into(),
            thread_id: "def456".into(),
            ..Default::default()
        };
        let mut notice =
            rule_notice("one@example.test", &message, crate::rules::Outcome::Email).unwrap();
        sender_image(
            &crate::store::Store::open(None).unwrap(),
            &message,
            &mut notice,
        );
        assert_eq!(notice["icon"], "gmailtjik");
        assert!(notice["image"].is_null());
        let link =
            crate::links::MailLink::parse(notice["actions"][0]["command"][2].as_str().unwrap())
                .unwrap();
        assert_eq!(link.message, message.id);
        assert_eq!(link.thread, message.thread_id);
        assert_eq!(link.account, "one@example.test");
    }
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
    fn auth_has_copy_and_open_actions_in_one_card() {
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
        assert_eq!(n["actions"].as_array().unwrap().len(), 2);
        assert_eq!(n["actions"][0]["copy"], "004281");
        assert_ne!(n["group"], "gmailtjik");
    }
}
