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
    /// List previews carry headers and snippets; their body must be fetched on open.
    #[serde(default)]
    pub summary_only: bool,
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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct DraftSlot {
    pub draft: Draft,
    pub pending_save: Option<i64>,
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
    #[serde(default)]
    pub threaded: bool,
}

pub struct Conversation<'a> {
    pub messages: Vec<&'a Message>,
}

impl Message {
    pub fn conversation_key(&self) -> &str {
        if self.thread_id.is_empty() {
            &self.id
        } else {
            &self.thread_id
        }
    }
}

impl Conversation<'_> {
    pub fn latest(&self) -> &Message {
        self.messages[self.messages.len() - 1]
    }

    // Prefer the latest received message as the reply target, even after a sent reply.
    pub fn message(&self) -> &Message {
        self.messages
            .iter()
            .rev()
            .find(|m| !m.label_ids.iter().any(|l| l == "SENT"))
            .copied()
            .unwrap_or_else(|| self.latest())
    }

    pub fn unread(&self) -> bool {
        self.messages.iter().any(|m| m.unread())
    }

    /// Only people who sent a message in this conversation.
    /// Prefer a named address when another header only supplies the email.
    pub fn avatar_participants(&self) -> Vec<(String, String)> {
        let mut participants: Vec<(String, String)> = Vec::new();
        for message in &self.messages {
            for header in ["From"] {
                for (name, email) in addresses(&message.header(header)).unwrap_or_default() {
                    if let Some(existing) = participants
                        .iter_mut()
                        .find(|(_, address)| address.eq_ignore_ascii_case(&email))
                    {
                        if existing.0.is_empty() && !name.is_empty() {
                            existing.0 = name;
                        }
                    } else {
                        participants.push((name, email));
                    }
                }
            }
        }
        participants
    }

    pub fn participants(&self, account: &str) -> String {
        let mut seen = std::collections::HashSet::new();
        let mut names = Vec::new();
        for message in &self.messages {
            let from = message.header("From");
            for (name, email) in addresses(&from).unwrap_or_else(|_| vec![(from.clone(), from)]) {
                if seen.insert(email.to_lowercase()) {
                    names.push(if email.eq_ignore_ascii_case(account) {
                        "me".into()
                    } else if name.is_empty() {
                        email
                    } else {
                        name
                    });
                }
            }
        }
        names.join(", ")
    }
}

impl Page {
    pub fn conversations(&self) -> Vec<Conversation<'_>> {
        let mut positions = std::collections::HashMap::new();
        let mut rows: Vec<Conversation<'_>> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for message in &self.messages {
            if !seen.insert(&message.id) {
                continue;
            }
            let next = rows.len();
            let index = *positions.entry(message.conversation_key()).or_insert(next);
            if index == next {
                rows.push(Conversation {
                    messages: Vec::new(),
                });
            }
            rows[index].messages.push(message);
        }
        for row in &mut rows {
            row.messages
                .sort_by_key(|m| m.internal_date.parse::<i64>().unwrap_or(0));
        }
        // Preserve Gmail's thread-list order, including search result order.
        rows
    }

    pub fn representatives(&self) -> Vec<Message> {
        self.conversations()
            .iter()
            .map(|row| row.message().clone())
            .collect()
    }

    pub fn selection(&self, current: Option<&Message>) -> Option<Message> {
        let rows = self.representatives();
        let mut selected = current
            .and_then(|current| {
                rows.iter()
                    .find(|row| row.conversation_key() == current.conversation_key())
            })
            .or_else(|| rows.first())?
            .clone();
        // A later list batch must not replace an already opened body with headers.
        if let Some(current) = current
            && current.id == selected.id
            && !current.summary_only
            && selected.summary_only
        {
            selected.payload = current.payload.clone();
            selected.summary_only = false;
        }
        Some(selected)
    }

    pub fn retain_conversations(&mut self, matches: impl Fn(&Message) -> bool) {
        let keep: std::collections::HashSet<String> = self
            .messages
            .iter()
            .filter(|m| matches(m))
            .map(|m| m.conversation_key().to_owned())
            .collect();
        self.messages
            .retain(|m| keep.contains(m.conversation_key()));
    }
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
            summary_only: false,
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
    if std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("signature") {
        messages[0].payload.mime_type = "text/html".into();
        messages[0].payload.body.data =
            URL_SAFE_NO_PAD.encode(include_str!("../assets/demo-signature.html"));
        messages[0].snippet =
            "Please review the sample; the delivery desk number belongs to the message.".into();
        for header in &mut messages[0].payload.headers {
            if header.name == "Subject" {
                header.value = "Studio delivery details".into();
            }
        }
    }
    if std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("inline") {
        messages[0].payload.mime_type = "multipart/alternative".into();
        messages[0].payload.body = Body::default();
        messages[0].payload.parts = vec![
            Part {
                mime_type: "text/plain".into(),
                body: Body { data: URL_SAFE_NO_PAD.encode("[image: first.png]\nBetween the pictures.\n[image: second.png]\nAfter both pictures.\n\nOn Monday, Mira wrote:\n> Older synthetic notes."), ..Body::default() },
                ..Part::default()
            },
            Part {
                mime_type: "text/html".into(),
                body: Body { data: URL_SAFE_NO_PAD.encode(include_str!("../assets/demo-inline.html")), ..Body::default() },
                ..Part::default()
            },
        ];
        messages[0].snippet = "Inline pictures stay between the message paragraphs.".into();
    }
    if std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("reader") {
        messages[0].payload.mime_type = "text/html".into();
        messages[0].payload.body.data =
            URL_SAFE_NO_PAD.encode(include_str!("../assets/demo-readable.html"));
        messages[0].snippet = "A simple message with consistent typography and lists.".into();
    }
    if std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("avatar-stack") {
        for n in 1..=6 {
            let mut reply = messages[0].clone();
            reply.id = format!("stack-reply-{n}");
            for header in &mut reply.payload.headers {
                if header.name == "From" {
                    header.value = format!("Person {n} <person{n}@example.test>");
                }
            }
            messages.push(reply);
        }
    }
    if std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("footer-actions") {
        messages[0].payload.mime_type = "text/html".into();
        let links = [
            "Website",
            "Privacy policy",
            "Email preferences",
            "Legal information",
            "Terms of Service",
            "Contact",
            "Community",
            "Support",
        ]
        .into_iter()
        .enumerate()
        .map(|(n, label)| format!("<a href=\"https://example.test/{n}\">{label}</a> "))
        .collect::<String>();
        messages[0].payload.body.data = URL_SAFE_NO_PAD.encode(format!("<table><tr><td>Account update</td><td>Privacy and preferences</td></tr><tr><td>Details</td><td>Synthetic notice</td></tr></table><p>{links}</p>"));
    }
    if std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("sender-details") {
        messages[0].payload.headers.push(Header {
            name: "List-Unsubscribe".into(),
            value: "<https://newsletter.example.test/unsubscribe?token=synthetic>, <mailto:unsubscribe@example.test?subject=unsubscribe>".into(),
        });
    }
    if std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("html") {
        messages[0].payload.mime_type = "text/html".into();
        messages[0].payload.body.data =
            URL_SAFE_NO_PAD.encode(include_str!("../assets/demo-email.html"));
        messages[0].snippet = "I've corrected the sample details below.".into();
        for header in &mut messages[0].payload.headers {
            if header.name == "Subject" {
                header.value = "Re: Studio sample details and artwork".into();
            }
        }
        let thread = demo_html_thread(&messages[0]);
        messages.splice(0..1, thread);
    }
    if matches!(
        std::env::var("GMAILTJIK_DEMO_SCENE").as_deref(),
        Ok("quick-folder" | "cache-failure" | "optimistic")
    ) {
        let sample = messages[0].clone();
        messages = (1..=25)
            .map(|index| {
                let mut message = sample.clone();
                message.id = format!("quick-message-{index}");
                message.thread_id = format!("quick-thread-{index}");
                message.snippet = format!("Preview for synthetic conversation {index:02}.");
                message.internal_date = (1793491200000_i64 - index * 60000).to_string();
                if std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("optimistic")
                    && index % 5 == 0
                {
                    message.label_ids.push("STARRED".into());
                }
                message.payload.body.data = URL_SAFE_NO_PAD.encode(format!(
                    "Full content of synthetic conversation {index:02}."
                ));
                for header in &mut message.payload.headers {
                    match header.name.as_str() {
                        "Subject" => header.value = format!("Quick folder {index:02}"),
                        "Message-ID" => header.value = format!("<quick-{index}@example.test>"),
                        _ => {}
                    }
                }
                message
            })
            .collect();
    }
    if std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("empty-folders") {
        labels.push(Label {
            id: "SPAM".into(),
            name: "Spam".into(),
            messages_unread: 0,
        });
        // Keep ordinary Inbox mail alongside read/unread messages in each folder.
        let sample = messages[0].clone();
        for (label, count) in [("SPAM", 3), ("TRASH", 2)] {
            for index in 0..count {
                let mut message = sample.clone();
                message.id = format!("empty-{label}-{index}");
                message.thread_id = message.id.clone();
                message.label_ids = vec![label.into()];
                if index == 0 {
                    message.label_ids.push("UNREAD".into());
                }
                for header in &mut message.payload.headers {
                    if header.name == "Subject" {
                        header.value = format!("Synthetic {label} message {}", index + 1);
                    }
                }
                messages.push(message);
            }
        }
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
            threaded: true,
        },
    )
}

/// Three synthetic conversations with 3, 1 and 2 messages, including a sent reply.
pub fn demo_conversations() -> (Vec<Label>, Page) {
    let (mut labels, mut page) = demo();
    let first = page.messages[0].clone();
    let mut reply = first.clone();
    reply.id = "demo-received-reply".into();
    reply.internal_date = "1793493000000".into();
    reply.label_ids.retain(|l| l != "UNREAD");
    for header in &mut reply.payload.headers {
        if header.name == "Subject" {
            header.value = format!("Re: {}", first.subject());
        }
    }
    let sent = demo_thread(&first).pop().unwrap();
    let single = page.messages[2].clone();
    let last = page.messages[3].clone();
    let mut last_reply = last.clone();
    last_reply.id = "demo-last-reply".into();
    last_reply.internal_date = "1793482200000".into();
    page.messages = vec![first, reply, sent, single, last, last_reply];
    for label in &mut labels {
        label.messages_unread = page
            .messages
            .iter()
            .filter(|m| m.unread() && m.label_ids.contains(&label.id))
            .count() as u64;
    }
    (labels, page)
}

pub fn demo_thread(message: &Message) -> Vec<Message> {
    if matches!(
        std::env::var("GMAILTJIK_DEMO_SCENE").as_deref(),
        Ok("reader" | "inline" | "signature")
    ) {
        return vec![message.clone()];
    }
    if message.id == "demo-0" && std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("html") {
        return demo_html_thread(message);
    }
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
        reply
            .payload
            .headers
            .retain(|h| !h.name.eq_ignore_ascii_case("List-Unsubscribe"));
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

/// A realistic reply chain for checking wrapping, collapsed rows and HTML rendering.
/// Every address, date, reference and message in this scene is synthetic.
fn demo_html_thread(latest: &Message) -> Vec<Message> {
    let history = [
        (
            "Mira Chen - Studio sample review <mira@example.test>",
            "Here is the first synthetic sample for review.\nThe artwork and sample details are attached below.",
            false,
        ),
        (
            "You <you@example.test>",
            "Could you check the sample information and send the complete artwork?\nThe delivery date needs a correction.",
            true,
        ),
        (
            "Mira Chen <mira@example.test>",
            "I'll update the date and send the full image.\nThe project reference is <https://example.test/studio/sample-details>.",
            false,
        ),
    ];
    let mut messages: Vec<_> = history
        .into_iter()
        .enumerate()
        .map(|(index, (from, body, sent))| {
            let mut message = latest.clone();
            message.id = format!("demo-html-history-{index}");
            message.internal_date = (1793491200000_i64 - (3 - index) as i64 * 3600000).to_string();
            message.label_ids = vec![if sent { "SENT" } else { "INBOX" }.into()];
            message.snippet = body.replace('\n', " ");
            message.payload.mime_type = "text/plain".into();
            message.payload.body.data = URL_SAFE_NO_PAD.encode(body);
            for header in &mut message.payload.headers {
                match header.name.as_str() {
                    "From" => header.value = from.into(),
                    "To" if sent => header.value = "Mira Chen <mira@example.test>".into(),
                    "Message-ID" => {
                        header.value = format!("<synthetic-history-{index}@example.test>")
                    }
                    _ => {}
                }
            }
            message
        })
        .collect();
    messages.push(latest.clone());
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_participants_exclude_non_senders_and_deduplicate_addresses() {
        let mut message = Message::default();
        message.payload.headers = vec![
            Header {
                name: "From".into(),
                value: "Sender <sender@example.test>".into(),
            },
            Header {
                name: "To".into(),
                value: "reader@example.test, Recipient <recipient@example.test>".into(),
            },
            Header {
                name: "Cc".into(),
                value: "Reader <READER@example.test>, Observer <observer@example.test>".into(),
            },
        ];
        let mut reply = message.clone();
        reply.payload.headers[0].value = "Reader <READER@example.test>".into();
        let conversation = Conversation {
            messages: vec![&message, &message, &reply],
        };
        assert_eq!(
            conversation.avatar_participants(),
            vec![
                ("Sender".into(), "sender@example.test".into()),
                ("Reader".into(), "READER@example.test".into()),
            ]
        );
    }

    #[test]
    fn progressive_previews_preserve_selection_and_opened_content() {
        let original = demo().1;
        let selected = original.messages[1].clone();
        let mut summaries = original.clone();
        for message in &mut summaries.messages {
            message.summary_only = true;
            message.payload.body = Body::default();
            message.payload.parts.clear();
            message.label_ids.retain(|label| label != "UNREAD");
        }
        let mut partial = summaries.clone();
        partial.messages.truncate(2);
        let kept = partial.selection(Some(&selected)).unwrap();
        assert_eq!(kept.id, selected.id);
        assert!(!kept.summary_only);
        assert_eq!(kept.text(), selected.text());
        assert!(!kept.unread());
        assert_eq!(summaries.selection(Some(&kept)).unwrap().id, selected.id);
        summaries.messages.remove(1);
        assert_eq!(
            summaries.selection(Some(&kept)).unwrap().id,
            original.messages[0].id
        );
        let roundtrip: Page =
            serde_json::from_str(&serde_json::to_string(&partial).unwrap()).unwrap();
        assert!(
            roundtrip
                .messages
                .iter()
                .all(|message| message.summary_only)
        );
        let legacy: Message = serde_json::from_str(r#"{"id":"old","threadId":"thread"}"#).unwrap();
        assert!(!legacy.summary_only);
    }

    #[test]
    fn conversations_include_sent_replies_and_aggregate_unread() {
        let (_, page) = demo_conversations();
        let rows = page.conversations();
        assert_eq!(
            rows.iter().map(|r| r.messages.len()).collect::<Vec<_>>(),
            [3, 1, 2]
        );
        assert_eq!(rows[0].participants("you@example.test"), "Mira Chen, me");
        assert!(rows[0].unread());
        assert!(!rows[0].message().unread());
        assert_eq!(rows[0].message().id, "demo-received-reply");
        assert_eq!(rows[0].latest().id, "demo-reply-0");
        assert_eq!(
            rows[0].messages[0].subject(),
            "A quieter place for your inbox"
        );
    }

    #[test]
    fn conversation_identity_is_gmail_id_not_subject() {
        let mut page = demo_conversations().1;
        let mut same_subject = page.messages[0].clone();
        same_subject.id = "different-message".into();
        same_subject.thread_id = "different-thread".into();
        page.messages.push(same_subject.clone());
        page.messages.push(same_subject); // Duplicate IDs do not inflate the count.
        assert_eq!(page.conversations().len(), 4);
        assert_eq!(page.conversations()[3].messages.len(), 1);
        for message in &mut page.messages {
            message.thread_id.clear();
        }
        assert_eq!(page.conversations().len(), 7);
    }
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
