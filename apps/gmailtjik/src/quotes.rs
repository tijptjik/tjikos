use crate::model::Message;

/// Collapse recognizable reply quotations without discarding inline replies.
pub fn body(message: &Message) -> (String, bool) {
    let full = message.text();
    let mut kept = Vec::new();
    let lines: Vec<_> = full.lines().collect();
    let mut hidden = false;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim_start();
        if line.starts_with('>') {
            hidden = true;
            index += 1;
            continue;
        }
        if line.starts_with("On ") {
            let end = (index + 5).min(lines.len());
            if let Some(last) = (index..end).find(|&n| lines[n].trim_end().ends_with("wrote:")) {
                if lines[last + 1..]
                    .iter()
                    .find(|line| !line.trim().is_empty())
                    .is_some_and(|line| line.trim_start().starts_with('>'))
                {
                    hidden = true;
                    index = last + 1;
                    continue;
                }
            }
        }
        if !message.header("In-Reply-To").is_empty()
            && (line.eq_ignore_ascii_case("-----Original Message-----")
                || line.eq_ignore_ascii_case("________________________________"))
            && lines[index + 1..]
                .iter()
                .take(8)
                .any(|line| line.trim_start().starts_with("From:"))
            && lines[index + 1..]
                .iter()
                .take(8)
                .any(|line| line.trim_start().starts_with("Subject:"))
        {
            hidden = true;
            break;
        }
        kept.push(lines[index]);
        index += 1;
    }
    if hidden {
        return (kept.join("\n").trim().to_string(), true);
    }

    // Some clients identify quote history only in the HTML alternative.
    let parts = message.payload.walk();
    let selector =
        scraper::Selector::parse(".gmail_quote, .yahoo_quoted, blockquote[type=cite]").unwrap();
    let mut texts = Vec::new();
    for part in parts
        .into_iter()
        .filter(|part| part.mime_type == "text/html" && part.filename.is_empty())
    {
        let mut html = scraper::Html::parse_document(&part.decoded_text());
        let quotes: Vec<_> = html.select(&selector).map(|node| node.id()).collect();
        hidden |= !quotes.is_empty();
        for id in quotes {
            if let Some(mut node) = html.tree.get_mut(id) {
                node.detach();
            }
        }
        if let Ok(text) = html2text::from_read(html.html().as_bytes(), 90) {
            texts.push(text);
        }
    }
    if hidden {
        (texts.join("\n\n").trim().into(), true)
    } else {
        (full, false)
    }
}

#[cfg(test)]
mod tests {
    use super::body;
    use crate::model::{Body, Message, Part};
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    fn mail(mime: &str, text: &str) -> Message {
        Message {
            payload: Part {
                mime_type: mime.into(),
                body: Body {
                    data: URL_SAFE_NO_PAD.encode(text),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }
    #[test]
    fn preserves_inline_answers_and_can_reveal_original() {
        let message = mail(
            "text/plain",
            "Yes!\n\nOn Monday, Mira wrote:\n> First question\nMy answer\n> Second question\nAnother answer",
        );
        let (text, hidden) = body(&message);
        assert!(hidden);
        assert_eq!(text, "Yes!\n\nMy answer\nAnother answer");
        assert!(message.text().contains("First question"));
    }
    #[test]
    fn collapses_html_quote_containers_but_keeps_new_content() {
        let message = mail(
            "text/html",
            "<p>New answer</p><div class='gmail_quote'>Old body</div>",
        );
        let (text, hidden) = body(&message);
        assert!(hidden);
        assert!(text.contains("New answer"));
        assert!(!text.contains("Old body"));
    }
    #[test]
    fn uses_html_quote_boundaries_when_plain_alternative_has_none() {
        let mut message = mail("multipart/alternative", "");
        message.payload.parts = vec![
            mail("text/plain", "New answer
Old body without quote markers").payload,
            mail("text/html", "<p>New answer<img src='https://example.test/new.png'></p><div class='gmail_quote'>Old body<img src='https://example.test/old.png'></div>").payload,
        ];
        let (text, hidden) = body(&message);
        assert!(hidden);
        assert!(text.contains("New answer"));
        assert!(!text.contains("Old body"));
        assert_eq!(message.remote_images().len(), 1);
        assert_eq!(message.remote_images()[0].0, "https://example.test/new.png");
    }
    #[test]
    fn leaves_unrecognized_prose_and_forwarded_mail_intact() {
        let message = mail(
            "text/plain",
            "On Monday we wrote:\nA fresh paragraph\n----- Forwarded message -----\nUseful forwarded content",
        );
        assert_eq!(body(&message), (message.text(), false));
    }
}
