use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub messages_unread: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub thread_id: String,
    #[serde(default)]
    pub label_ids: Vec<String>,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub internal_date: String,
    #[serde(default)]
    pub payload: Part,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub headers: Vec<Header>,
    #[serde(default)]
    pub body: Body,
    #[serde(default)]
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Body {
    #[serde(default)]
    pub data: String,
    #[serde(default)]
    pub attachment_id: String,
    #[serde(default)]
    pub size: usize,
}

impl Part {
    pub fn header(&self, name: &str) -> String {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| {
                let raw = format!("{}: {}", h.name, h.value);
                mailparse::parse_header(raw.as_bytes())
                    .map(|(header, _)| header.get_value())
                    .unwrap_or_else(|_| h.value.clone())
            })
            .unwrap_or_default()
    }

    pub fn walk(&self) -> Vec<&Part> {
        let mut parts = vec![self];
        for part in &self.parts {
            parts.extend(part.walk());
        }
        parts
    }

    pub fn decoded(&self) -> Vec<u8> {
        URL_SAFE_NO_PAD
            .decode(self.body.data.trim_end_matches('='))
            .unwrap_or_default()
    }
    pub fn decoded_text(&self) -> String {
        let content_type = mailparse::parse_content_type(&self.header("Content-Type"));
        let encoding = content_type
            .params
            .get("charset")
            .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
            .unwrap_or(encoding_rs::UTF_8);
        encoding.decode(&self.decoded()).0.into_owned()
    }
}

impl Message {
    pub fn header(&self, name: &str) -> String {
        self.payload.header(name)
    }
    pub fn subject(&self) -> String {
        let subject = self.header("Subject");
        if subject.is_empty() {
            "(No subject)".into()
        } else {
            subject
        }
    }
    pub fn unread(&self) -> bool {
        self.label_ids.iter().any(|l| l == "UNREAD")
    }
    pub fn text(&self) -> String {
        let parts = self.payload.walk();
        let plain: Vec<_> = parts
            .iter()
            .filter(|p| p.mime_type == "text/plain" && p.filename.is_empty())
            .map(|p| p.decoded_text())
            .collect();
        if !plain.is_empty() {
            return plain.join("\n\n");
        }
        parts
            .iter()
            .filter(|p| p.mime_type == "text/html" && p.filename.is_empty())
            .filter_map(|p| html2text::from_read(p.decoded_text().as_bytes(), 90).ok())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
    pub fn remote_images(&self) -> Vec<(String, String)> {
        let selector = scraper::Selector::parse("img[src]").unwrap();
        let quotes =
            scraper::Selector::parse(".gmail_quote, .yahoo_quoted, blockquote[type=cite]").unwrap();
        self.payload
            .walk()
            .iter()
            .filter(|p| p.mime_type == "text/html")
            .flat_map(|p| {
                let html = scraper::Html::parse_document(&p.decoded_text());
                html.select(&selector)
                    .filter_map(|img| {
                        if img
                            .ancestors()
                            .filter_map(scraper::ElementRef::wrap)
                            .any(|node| quotes.matches(&node))
                        {
                            return None;
                        }
                        let src = img.value().attr("src")?;
                        let url = url::Url::parse(src).ok()?;
                        (url.scheme() == "https").then(|| {
                            (
                                url.to_string(),
                                img.value().attr("alt").unwrap_or("Email image").to_string(),
                            )
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .take(20)
            .collect()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Draft {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    pub thread_id: String,
    pub in_reply_to: String,
    pub references: String,
    pub gmail_id: String,
}

impl Draft {
    pub fn reply(message: &Message) -> Self {
        let reply_to = message.header("Reply-To");
        let subject = message.subject();
        let message_id = message.header("Message-ID");
        Self {
            to: if reply_to.is_empty() {
                message.header("From")
            } else {
                reply_to
            },
            subject: if subject.to_lowercase().starts_with("re:") {
                subject
            } else {
                format!("Re: {subject}")
            },
            thread_id: message.thread_id.clone(),
            in_reply_to: message_id.clone(),
            references: format!("{} {}", message.header("References"), message_id)
                .trim()
                .into(),
            ..Self::default()
        }
    }
    pub fn validate(&self) -> Result<()> {
        for field in [
            &self.to,
            &self.cc,
            &self.bcc,
            &self.subject,
            &self.in_reply_to,
            &self.references,
        ] {
            if field.contains(['\r', '\n']) {
                bail!("Mail headers must be single-line");
            }
        }
        if self.to.trim().is_empty() && self.cc.trim().is_empty() && self.bcc.trim().is_empty() {
            bail!("Add at least one recipient");
        }
        for value in [&self.to, &self.cc, &self.bcc] {
            addresses(value)?;
        }
        Ok(())
    }
    pub fn raw(&self, from: &str) -> Result<String> {
        self.validate()?;
        let mut mail = mail_builder::MessageBuilder::new()
            .from(from)
            .to(addresses(&self.to)?)
            .subject(self.subject.clone())
            .text_body(self.body.clone());
        if !self.cc.trim().is_empty() {
            mail = mail.cc(addresses(&self.cc)?);
        }
        if !self.bcc.trim().is_empty() {
            mail = mail.bcc(addresses(&self.bcc)?);
        }
        if !self.in_reply_to.is_empty() {
            mail = mail.in_reply_to(self.in_reply_to.trim_matches(['<', '>']).to_string());
        }
        if !self.references.is_empty() {
            mail = mail.references(
                self.references
                    .split_whitespace()
                    .map(|id| id.trim_matches(['<', '>']).to_string())
                    .collect::<Vec<_>>(),
            );
        }
        Ok(URL_SAFE_NO_PAD.encode(mail.write_to_vec()?))
    }
}

pub fn addresses(value: &str) -> Result<Vec<(String, String)>> {
    if value.trim().is_empty() {
        return Ok(vec![]);
    }
    let parsed = mailparse::addrparse(value)?;
    let mut out = Vec::new();
    for addr in parsed.iter() {
        let mailparse::MailAddr::Single(addr) = addr else {
            bail!("Use individual email addresses rather than address groups");
        };
        let Some((local, domain)) = addr.addr.rsplit_once('@') else {
            bail!("Invalid email address: {}", addr.addr);
        };
        if local.is_empty() || domain.is_empty() || addr.addr.chars().any(char::is_whitespace) {
            bail!("Invalid email address");
        }
        out.push((
            addr.display_name.clone().unwrap_or_default(),
            addr.addr.clone(),
        ));
    }
    Ok(out)
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Contact {
    /// Google People identity within this account-specific cache.
    #[serde(default)]
    pub resource_name: String,
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub photo: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Page {
    pub messages: Vec<Message>,
    pub next: String,
}

pub fn demo() -> (Vec<Label>, Page) {
    let mut labels: Vec<Label> = [
        ("INBOX", "Inbox"),
        ("STARRED", "Starred"),
        ("SENT", "Sent"),
        ("DRAFT", "Drafts"),
        ("design", "Mail/Design"),
        ("projects", "Mail/Design/Projects"),
        ("code", "Mail/Code"),
        ("statements", "Finances/Statements"),
        ("transactions", "Finances/Transactions"),
        ("travel", "Travel"),
        ("receipts", "Finances/Receipts"),
        ("TRASH", "Trash"),
    ]
    .into_iter()
    .map(|(id, name)| Label {
        messages_unread: 0,
        id: id.into(),
        name: name.into(),
    })
    .collect();
    let rows = [
        (
            "Mira Chen <mira@example.test>",
            "A quieter place for your inbox",
            "A small note about the new direction",
            "Hey,\n\nI pulled together the first notes for our new mail client. A quiet sidebar, a fast inbox, and room to read.\n\nThe important part: opening an email should feel immediate. Keep the images honest, and let the typography breathe.\n\nCould you send me your thoughts before Thursday?\n\nMira",
            "design",
        ),
        (
            "Jun Park <jun@example.test>",
            "Weekend by the sea",
            "The little place we talked about is available",
            "The house is available next weekend. Shall we book two nights?\n\nJun",
            "travel",
        ),
        (
            "Studio North <hello@example.test>",
            "November studio notes",
            "New sketches, familiar colours",
            "Our November sketches are ready. Thank you for taking a look.\n\nStudio North",
            "design",
        ),
        (
            "Paper Shop <orders@example.test>",
            "Your order is on its way",
            "A little something for your desk",
            "Your notebook order has shipped.\n\nOrder number: DEMO-1042",
            "receipts",
        ),
        (
            "Alex Rivera <alex@example.test>",
            "Re: Lunch next week",
            "Tuesday works well for me",
            "Tuesday works well. Let's meet at noon by the station.\n\nAlex",
            "INBOX",
        ),
    ];
    let mut messages: Vec<Message> = rows
        .into_iter()
        .enumerate()
        .map(|(i, (from, subject, snippet, body, label))| Message {
            id: format!("demo-{i}"),
            thread_id: format!("thread-{i}"),
            label_ids: vec!["INBOX".into(), label.into()]
                .into_iter()
                .chain((i < 2).then(|| "UNREAD".into()))
                .collect(),
            snippet: snippet.into(),
            internal_date: (1793491200000_i64 - i as i64 * 3600000).to_string(),
            payload: Part {
                mime_type: "text/plain".into(),
                headers: [
                    ("From", from),
                    ("To", "You <you@example.test>"),
                    ("Subject", subject),
                    ("Message-ID", "<synthetic@example.test>"),
                ]
                .into_iter()
                .map(|(name, value)| Header {
                    name: name.into(),
                    value: value.into(),
                })
                .collect(),
                body: Body {
                    data: URL_SAFE_NO_PAD.encode(body),
                    ..Body::default()
                },
                ..Part::default()
            },
        })
        .collect();
    if std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("html") {
        messages[0].payload.mime_type = "text/html".into();
        messages[0].payload.body.data =
            URL_SAFE_NO_PAD.encode(include_str!("../assets/demo-email.html"));
    }
    for label in &mut labels {
        label.messages_unread = messages
            .iter()
            .filter(|message| message.unread() && message.label_ids.contains(&label.id))
            .count() as u64;
    }
    (
        labels,
        Page {
            messages,
            next: String::new(),
        },
    )
}

pub fn demo_thread(message: &Message) -> Vec<Message> {
    let mut messages = vec![message.clone()];
    if message.id == "demo-0" {
        let mut reply = message.clone();
        reply.id = "demo-reply-0".into();
        reply.internal_date = "1793494800000".into();
        reply.label_ids = vec!["SENT".into()];
        for header in &mut reply.payload.headers {
            if header.name == "From" {
                header.value = "You <you@example.test>".into();
            }
            if header.name == "To" {
                header.value = "Mira <mira@example.test>".into();
            }
        }
        reply.payload.mime_type = "text/plain".into();
        reply.payload.body.data = URL_SAFE_NO_PAD.encode(format!(
            "Thanks, Mira. I will send notes before Thursday.

On Sunday, Mira wrote:
{}",
            message
                .text()
                .lines()
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join(
                    "
"
                )
        ));
        messages.push(reply);
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_mime_headers_and_legacy_text_charsets() {
        let part = Part {
            headers: vec![
                Header {
                    name: "Subject".into(),
                    value: "=?UTF-8?B?5L2g5aW9?=".into(),
                },
                Header {
                    name: "Content-Type".into(),
                    value: "text/plain; charset=iso-8859-1".into(),
                },
            ],
            body: Body {
                data: URL_SAFE_NO_PAD.encode(b"caf\xe9"),
                ..Body::default()
            },
            ..Part::default()
        };
        assert_eq!(part.header("Subject"), "你好");
        assert_eq!(part.decoded_text(), "café");
    }
    #[test]
    fn header_injection_is_rejected() {
        let draft = Draft {
            to: "a@example.test\r\nBcc: thief@example.test".into(),
            ..Draft::default()
        };
        assert!(draft.raw("me@example.test").is_err());
    }
    #[test]
    fn reply_preserves_thread_and_prefers_reply_to() {
        let mut msg = demo().1.messages.remove(0);
        msg.payload.headers.push(Header {
            name: "Reply-To".into(),
            value: "reply@example.test".into(),
        });
        let reply = Draft::reply(&msg);
        assert_eq!(reply.to, "reply@example.test");
        assert_eq!(reply.thread_id, msg.thread_id);
        assert_eq!(reply.subject, "Re: A quieter place for your inbox");
        let raw = URL_SAFE_NO_PAD
            .decode(reply.raw("you@example.test").unwrap())
            .unwrap();
        let parsed = mailparse::parse_mail(&raw).unwrap();
        use mailparse::MailHeaderMap;
        assert_eq!(
            parsed.headers.get_first_value("In-Reply-To").unwrap(),
            "<synthetic@example.test>"
        );
    }
    #[test]
    fn encoded_mail_has_unicode_and_recipients() {
        let draft = Draft {
            to: "Mira <mira@example.test>".into(),
            bcc: "b@example.test".into(),
            subject: "你好".into(),
            body: "Hello\n世界".into(),
            ..Draft::default()
        };
        let raw = URL_SAFE_NO_PAD
            .decode(draft.raw("you@example.test").unwrap())
            .unwrap();
        let mail = mailparse::parse_mail(&raw).unwrap();
        use mailparse::MailHeaderMap;
        assert_eq!(mail.headers.get_first_value("Subject").unwrap(), "你好");
        assert!(mail.get_body().unwrap().contains("世界"));
    }
}
